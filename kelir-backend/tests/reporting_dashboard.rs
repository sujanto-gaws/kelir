//! The dashboard summary: what is waiting for the person looking at it
//! (FR-RPT-001, [#431]), and every widget since, down to FR-RPT-006's approval
//! time card ([#461]).
//!
//! # Every assertion here reaches the rows
//!
//! [#431] AC3 asks for the viewer's scoping to be **in the statement rather than
//! in the handler**, which is the [#106]/[#121] lesson this project paid three
//! sprints of coverage findings for: *a test that asserts around a query proves
//! something about the handler and nothing about the rows.* So every scope test
//! below puts the row that must **not** be counted into the database and reads
//! the number back — a second user's draft, a second tenant's draft, a
//! submitted document, a soft-deleted one, a document with no author, another
//! role's task, a document only somebody else touched, and an activity event
//! belonging to another tenant.
//!
//! **One subject cannot tell scoped from unscoped**, and the assertion reads
//! identically either way (coding standard §2.9, [#218]'s single root cause), so
//! the fixture carries a second user and a second role wherever it asserts a
//! scope.
//!
//! # The ones that are not about this module
//!
//! [`the_summary_and_the_inbox_report_the_same_number`] and
//! [`the_widget_lists_the_rows_the_inbox_lists`] are the tests that would catch
//! the failure this module's shape exists to prevent — the first on the count,
//! the second on the rows. **Both read the inbox in the same test and assert
//! the dashboard against what it answered**, rather than against a number the
//! fixture wrote down, which is what [#432] AC2 means by a *divergence* test: a
//! task one surface lists and the other does not is the defect, whichever way
//! round it falls. A test that merely asserted the widget returned some rows
//! would pass for a widget that had grown a visibility rule of its own.
//!
//! [#432]: https://github.com/sujanto-gaws/kelir/issues/432
//!
//! The dashboard neither counts nor lists tasks its own way — both come through
//! `workflow::repository::inbox`'s statement, the one the inbox pages on — and
//! [#279](https://github.com/sujanto-gaws/kelir/issues/279) is this project's
//! record of what the alternative costs: the one duplicated predicate in that
//! file drifted, and an inbox said 23 and ended at 19. **These tests fail if
//! somebody gives the dashboard a query of its own**, which is the only way the
//! two surfaces can ever disagree.
//!
//! # Seen to fail (coding standard §2.9)
//!
//! **Seven mutations, run 2026-09-12, each red and each reddening its own test
//! and no other.** Stated as what was run rather than as what would be run, and
//! dated, which is what coding standard §2.9 requires of a claim of evidence
//! ([#404](https://github.com/sujanto-gaws/kelir/issues/404)). Baseline first:
//! **11 passed, nothing mutated.**
//!
//! | Mutation | Reddened |
//! |---|---|
//! | **M1** — `service::dashboard_summary`'s `caller.require(DASHBOARD_READ)` deleted | *the summary refuses a caller without the grant* |
//! | **M2** — `count_own_drafts` stops scoping to the author | *drafts are the caller's own, not the tenant's* |
//! | **M3** — `count_own_drafts` stops scoping to the tenant | *a second tenant's draft is not on this caller's dashboard* |
//! | **M4** — `count_own_drafts` stops filtering to `DRAFT` | *a submitted document has left the drafts count* |
//! | **M5** — `count_own_drafts` stops honouring the soft delete | *a deleted draft is not counted* |
//! | **M6** — `count_waiting`'s `InboxScope::Open` changed to `InboxScope::All` | *a finished task has left the waiting count* |
//! | **M7** — `count_waiting`'s overdue read taken with `InboxScope::Open` | *overdue counts only what is late* |
//!
//! **M6 survived the first run, and that is the finding worth recording.** On
//! the first pass this file had ten tests and none of them had a *finished*
//! task in its fixture, so `open` and `all` were the same set and the scope was
//! not under test at all: every assertion about the waiting count was really an
//! assertion about *whose* task it is, and the *is it still waiting* half was
//! uncovered. [`a_finished_task_has_left_the_waiting_count`] was written to
//! close it and M6 then reddened exactly that test.
//!
//! **A run that had only asked "did something go red" would have called the
//! first pass a clean sweep** — six of seven red looks like a good result and
//! the seventh was the one that mattered. That is why §2.9 asks for the
//! reddened test to be *named*.
//!
//! ## FR-RPT-002, the pending-task widget ([#432])
//!
//! **Five mutations, run 2026-09-12, each red and each reddening exactly one
//! test.** Baseline first: **16 passed, nothing mutated.**
//!
//! | Mutation | Reddened |
//! |---|---|
//! | **M1** — the waiting count taken from the page's length instead of the statement's `count(*) OVER ()` | [`the_card_shows_the_top_of_the_queue_and_counts_the_whole_of_it`] |
//! | **M2** — the paged statement's candidate arm stops scoping to `candidate_department_id` | [`a_department_scoped_task_reaches_only_that_departments_widget`] |
//! | **M3** — `waiting_work` reads `InboxScope::All` rather than `Open` | [`a_finished_task_has_left_the_waiting_count`] |
//! | **M4** — the overdue window counts every matched row rather than the late ones | [`overdue_counts_only_what_is_late`] |
//! | **M5** — the paged statement stops admitting a task by its `assignee_user_id` | [`a_delegated_task_is_on_the_delegates_widget_and_not_the_delegators`] |
//!
//! **M2 and M5 were rejected on the first attempt, and it is the anchor rule
//! working rather than a nuisance.** The visibility predicate is written three
//! times in `repository::inbox` — the page, the count and the detail gate — so
//! both mutations matched in more than one place and the runner refused them
//! instead of picking one. §2.9 names that exact failure: *`if !verified {`
//! occurring twice in one file is how a mutation lands on the wrong branch,
//! leaving the test green while nothing beneath it changed.* Here it would have
//! been worse than green — a mutation landing on `count_for_caller` would have
//! reddened a **different** test and been recorded as covering the widget's
//! rows. Both were re-anchored from the `LEFT JOIN users f` line, which only the
//! paged statement has, and then reddened the two tests above.
//!
//! **M1 is the one worth keeping in view.** The count and the rows now come from
//! one statement, so the way they can diverge is no longer a second query — it
//! is somebody deriving the count from the list, which reads as a
//! simplification and is wrong by exactly the number of rows the card is
//! hiding.
//!
//! ## FR-RPT-003, the recent-documents widget ([#433])
//!
//! **Eight mutations, run 2026-09-12.** Baseline first: **27 passed, nothing
//! mutated.** Seven were red on the first pass; **M3 survived**, and closing it
//! is what took the file to 28.
//!
//! | Mutation | Reddened |
//! |---|---|
//! | **M1** — the statement stops scoping to the caller (`a.actor_user_id = $2` widened to *any actor*) | [`the_widget_lists_the_documents_the_caller_raised`], and three more |
//! | **M2** — the statement stops honouring the soft delete | [`a_soft_deleted_document_is_not_recent`] |
//! | **M3** — the join stops carrying the tenant across to the event | **nothing, on the first run** — see below |
//! | **M4** — `max(a.created_at)` taken as `min` | [`the_rows_carry_the_timestamp_they_were_ordered_by`] |
//! | **M5** — the list ordered oldest touch first | [`the_newest_touch_decides_the_order`], and four more |
//! | **M6** — every event type counts as a touch, reads included | [`opening_a_file_does_not_make_a_document_recent`] |
//! | **M7** — `RECENT_DOCUMENTS_SHOWN` raised from 5 to 50 | [`the_card_shows_five_and_drops_the_oldest_touch`] |
//! | **M8** — `recent_documents` acquires a `document:read` check | [`the_widget_asks_for_no_document_read`], and two more |
//!
//! **M3 is the finding, and it is the same shape as item 3's M6.** Deleting
//! `a.tenant_id = d.tenant_id` from the join left all twenty-seven tests green.
//! [`a_second_tenants_document_is_not_recent_here`] was the test that should
//! have caught it and **could not have**: it puts the foreign event on a foreign
//! *document*, and `d.tenant_id = $1` excludes that document before the join is
//! reached. It proves the `WHERE` and says nothing about the `ON`.
//!
//! The row that reaches the join is the **mismatched** one — an event stamped
//! with another tenant, pointing at a document in this one — and
//! `activity_events` has no composite foreign key forbidding it
//! (`0033_activity.sql`). What it costs is not a leaked row but a **wrong
//! date**: the document is this tenant's and appears either way, while the
//! foreign event wins the `max(a.created_at)` and re-floats it to the top of
//! somebody's widget on activity recorded elsewhere.
//! [`a_foreign_tenants_event_cannot_date_this_tenants_document`] asserts the
//! order and the timestamp rather than presence, because presence is exactly
//! where the damage would not show, and M3 then reddened that test and no other.
//!
//! **M1, M5 and M8 redden more than one test each, and that is recorded rather
//! than tuned away.** Each is a predicate several tests rest on — *whose events
//! these are*, the order, and the absence of a second permission — so a
//! mutation to it ought to break more than one thing. What §2.9 asks is that
//! the *named* test is the one written for that predicate, and for all three it
//! is the first in the row.
//!
//! **M2, M4, M6, M7 and the re-run of M3 each reddened exactly one test**, which
//! is what makes the middle of this table load-bearing rather than
//! reassuring.
//!
//! [#433]: https://github.com/sujanto-gaws/kelir/issues/433
//!
//! ## FR-RPT-005, the overdue-tasks widget ([#446])
//!
//! **Eight mutations, run 2026-09-14, and all eight red** — seen red,
//! 2026-09-14. Baseline first: **36 passed, nothing mutated.** None survived,
//! so no test in this section was written to close one.
//!
//! | Mutation | Reddened |
//! |---|---|
//! | **M1** — the paged statement's `MostOverdueFirst` key sorted `DESC` | [`the_task_late_longest_comes_first`], and two more |
//! | **M2** — `late_work` reads `InboxScope::Open` rather than `Overdue` | [`an_undated_task_and_a_finished_late_one_are_not_overdue`], and three more |
//! | **M3** — `OVERDUE_TASKS_SHOWN` raised from 5 to 50 | [`the_card_shows_the_five_longest_late_and_counts_all_six`] |
//! | **M4** — `tasks_overdue` taken from `late.next.len()` instead of the statement's `count(*) OVER ()` | [`the_card_shows_the_five_longest_late_and_counts_all_six`] |
//! | **M5** — the `MostOverdueFirst` branch never taken (`CASE WHEN $11 AND false`) | [`the_task_late_longest_comes_first`], and one more |
//! | **M6** — `late_work` called with `waiting_work`'s filters, `Open` and `Newest` | [`the_overdue_widget_lists_what_the_inbox_calls_overdue`], and five more |
//! | **M7** — the paged statement's candidate arm stops scoping to `candidate_department_id` | [`a_department_scoped_late_task_reaches_only_that_departments_overdue_widget`], and its FR-RPT-002 twin |
//! | **M8** — the paged statement stops admitting a task by its `assignee_user_id` | [`a_delegated_late_task_is_on_the_delegates_overdue_widget_and_not_the_delegators`], and its FR-RPT-002 twin |
//!
//! **The four SQL mutations are anchored on lines only the paged statement
//! has**, which is the anchor rule the FR-RPT-002 table records paying for. M1
//! and M5 land on the `ORDER BY CASE WHEN $11` line, which exists once; M7 and
//! M8 are anchored from the `LEFT JOIN users f` line, as that table's M2 and M5
//! were, because the visibility predicate is written three times in
//! `repository::inbox` and a mutation landing on `count_for_caller` would
//! redden a different test and read as coverage of this one. A mutated
//! statement is not in `.sqlx`, so those four were compiled against a live
//! schema.
//!
//! **M3 and M4 are the pair worth keeping in view.** They are the two ways *N
//! more late* goes wrong — a card that quietly grows, and a count derived from
//! the card — and each reddens exactly one test, the same one. Only a fixture
//! with more late tasks than the card holds can tell either from correct, which
//! is why that test has six.
//!
//! **M1, M2, M5 and M6 redden more than one test each, and that is recorded
//! rather than tuned away.** Order and scope are what several tests here rest
//! on. The one worth naming is M2 reddening [`overdue_counts_only_what_is_late`]
//! — FR-RPT-001's test — because `tasksOverdue` now comes from `late_work`: the
//! count moved, and the older test followed it without being edited.
//!
//! **M7 and M8 redden a widget test and its FR-RPT-002 twin, and no other**,
//! which is [#446] AC3's *inherited, not restated* observed rather than
//! asserted: one predicate, one mutation, both cards.
//!
//! **One mutation was not run, and the reason is written as an intention.**
//! `NULLS LAST` changed to `NULLS FIRST` would stay green, because no undated
//! row reaches the sort key under `Overdue` — an equivalent mutant rather than
//! a gap, so it is not listed as coverage.
//!
//! **M1, M3, M5 and M7 came back with no result on the first pass**: the
//! previous run's test binary was still locked when the next one linked
//! (`LNK1104`). That is not a green, and it was not recorded as one; each was
//! run again and reddened as tabled.
//!
//! [#446]: https://github.com/sujanto-gaws/kelir/issues/446
//!
//! ## FR-RPT-004, the documents-by-status widget ([#447])
//!
//! **Seven mutations, run 2026-09-14, and all seven red** — seen red,
//! 2026-09-14. Baseline first: **42 passed, nothing mutated**, and the fifteen
//! `document::domain::status` unit tests beside them. None survived, so no test
//! in this section was written to close one.
//!
//! | Mutation | Reddened |
//! |---|---|
//! | **M1** — `count_own_by_status` stops scoping to the tenant (`tenant_id = $1` → `$1::uuid IS NOT NULL`) | [`a_second_tenants_documents_are_not_in_the_status_summary`], and its drafts twin |
//! | **M2** — `count_own_by_status` stops scoping to the author (`created_by = $2` → `$2::uuid IS NOT NULL`) | [`another_authors_documents_are_not_in_the_callers_status_summary`], and two more |
//! | **M3** — `count_own_by_status` admits a null author (`created_by = $2 OR created_by IS NULL`) | [`a_document_nobody_raised_is_in_nobodys_status_summary`] |
//! | **M4** — `count_own_by_status` stops honouring the soft delete | [`a_soft_deleted_document_is_not_in_the_status_summary`], and its drafts twin |
//! | **M5** — `DocumentStatusCount::every_status` drops the zeros | [`the_status_summary_counts_the_callers_documents_on_a_fixed_axis`], five more, and two unit tests |
//! | **M6** — `every_status` fills in reverse declaration order | the same six, and the same two unit tests |
//! | **M7** — `draftDocuments` read off the `RETURNED` entry | [`draft_documents_is_the_draft_entry_of_the_status_summary`], and the four drafts tests |
//!
//! **M3 is the one worth keeping in view, and the only one that reddened
//! exactly one test.** No fixture in this file had a document without an author
//! before [#447], so *the caller's, or nobody's* was never under test for the
//! drafts count either — its comment said a null author was counted for nobody
//! and nothing checked it. The test was written as one of AC2's subjects rather
//! than after a survivor, but it is now the only thing between that looser
//! predicate and every system-raised document appearing on every dashboard.
//!
//! **M2 reddens the null-author test as well**, which is the predicate being
//! honest rather than the test being loose: dropping `created_by` admits the
//! authorless row along with the other author's.
//!
//! **A "drafts twin" is the FR-RPT-001 test on the same predicate** —
//! [`a_second_tenants_draft_is_not_on_this_callers_dashboard`] for M1,
//! [`drafts_are_the_callers_own_not_the_tenants`] for M2,
//! [`a_deleted_draft_is_not_counted`] for M4. They were not written against
//! this statement and went red on it anyway, because `draftDocuments` now comes
//! from it: the count moved and the older tests followed it without being
//! edited, as the FR-RPT-005 table's M2 records for `tasksOverdue`. M7 reddens
//! all four drafts tests for the same reason.
//!
//! **M5 and M6 redden every test that asserts a whole axis** — six, because each
//! compares the served array with all ten entries — and the two fill tests in
//! `document::domain::status` beside them, so the zeros and the order are
//! covered below the endpoint as well as through it.
//!
//! **The four SQL mutations were anchored on the four-line block only
//! `count_own_by_status` has** — `deleted_at IS NULL` directly above
//! `GROUP BY status` — and the runner refused any anchor that did not occur
//! exactly once. A mutated statement is not in `.sqlx`, so those four compiled
//! against a live schema.
//!
//! **One mutation was not run, and it is an equivalent mutant rather than a
//! gap.** `entry.count += count` changed to `=` stays green by construction: a
//! `GROUP BY status` cannot return one status twice.
//!
//! **The FR-RPT-001 table above names `count_own_drafts`, which [#447]
//! removed.** Those rows record what was run on 2026-09-12 and are left as
//! written. Their M3, M2 and M5 predicates now live in `count_own_by_status` and
//! were run again as this table's M1, M2 and M4; their M4, *stops filtering to
//! `DRAFT`*, is this table's M7, because the filter is now which entry
//! `draftDocuments` is read from.
//!
//! [#447]: https://github.com/sujanto-gaws/kelir/issues/447
//!
//! ## FR-RPT-006, the approval time card ([#461])
//!
//! **Eighteen mutations, run across midnight 2026-09-16 to 2026-09-17, and all
//! eighteen red** — M1–M10 seen red, 2026-09-16; M11–M14 and D1–D4 seen red,
//! 2026-09-17. Baseline first: **56 passed, nothing mutated**, and the five
//! `workflow::domain::approval_time` unit tests beside them. None survived at
//! the level the predicate lives at, so no test in this section was written to
//! close one; **D2 is green through the endpoint**, and that is recorded below
//! rather than tidied away.
//!
//! | Mutation | Reddened |
//! |---|---|
//! | **M1** — the statement stops scoping the instance to the tenant (`i.tenant_id = $1` → `$1::uuid IS NOT NULL`) | [`a_second_tenants_documents_are_not_timed_here`] |
//! | **M2** — the join stops carrying the tenant across to the document | [`another_tenants_document_is_not_timed_through_an_instance_filed_here`] |
//! | **M3** — the statement stops scoping to the author (`d.created_by = $2` → `$2::uuid IS NOT NULL`) | [`another_authors_documents_are_not_timed_for_the_caller`] |
//! | **M4** — the statement admits a null author | [`another_authors_documents_are_not_timed_for_the_caller`] |
//! | **M5** — the document's soft delete dropped | [`a_soft_deleted_document_is_not_timed`] |
//! | **M6** — the instance's soft delete dropped | [`a_soft_deleted_instance_is_not_timed`] |
//! | **M7** — the deciding instance taken as the earliest (`started_at ASC, id ASC`) | [`a_document_still_in_flight_is_not_timed`], [`a_document_whose_last_instance_was_cancelled_is_not_timed`], [`a_document_decided_over_two_instances_is_timed_from_the_first`] |
//! | **M8** — `DISTINCT ON` dropped, so every instance is a row | [`a_document_still_in_flight_is_not_timed`], [`a_document_whose_last_instance_was_cancelled_is_not_timed`] |
//! | **M9** — a `CANCELLED` outcome counts as a decision | [`a_document_whose_last_instance_was_cancelled_is_not_timed`] |
//! | **M10** — a `REJECTED` outcome stops counting | [`the_approval_time_is_the_median_and_the_slowest_of_the_callers_decided_documents`] |
//! | **M11** — the clock starts at the latest instance (`min(started_at)` → `max`) | [`a_document_decided_over_two_instances_is_timed_from_the_first`] |
//! | **M12** — the window read the wrong way round (`>=` → `<=`) | twelve of the fourteen — every test with a recent decision |
//! | **M13** — `approval_time` passes the statement ten times the window it serves | [`a_decision_older_than_ninety_days_is_not_timed`] |
//! | **M14** — `approval_time` acquires `caller.require(INSTANCE_READ)` | [`the_approval_time_is_timed_under_the_dashboard_grant_alone`], and six earlier tests whose callers hold no `workflow:instance:read` |
//! | **D1** — the times are not sorted before the middle is read | both median unit tests; [`the_approval_time_is_the_median_and_the_slowest_of_the_callers_decided_documents`] |
//! | **D2** — an even count takes the lower middle rather than the mean | *an even count takes the mean of the middle two, rounded down* (unit); **green through the endpoint** |
//! | **D3** — the slowest taken as the fastest | both median unit tests; [`the_approval_time_is_the_median_and_the_slowest_of_the_callers_decided_documents`] |
//! | **D4** — nothing decided served as a zero-second median | both empty unit tests; [`a_caller_with_nothing_decided_has_nothing_timed`] |
//!
//! **M2 is the one worth keeping in view**, and it is the FR-RPT-003 table's M3
//! written down before the mutation rather than found after it.
//! `workflow_instances` references `documents (id)` alone, so an instance can
//! carry one tenant and point at another's document. M1's test cannot see
//! that: its foreign instance is foreign too, and `i.tenant_id = $1` drops it
//! first. Each of the two tenant predicates reddens its own test and no other.
//!
//! **M7 and M8 are the two halves of *the latest instance decides*.** M8 is
//! green on the two-instance test, and that is an equivalent mutant on that
//! fixture rather than a gap: its earlier round ended `RETURNED`, which the
//! outcome filter drops whether or not each document is one row, and the
//! window function still starts the clock at the first round. The in-flight and
//! cancelled tests each carry a document **whose earlier round was decided**,
//! which is the only shape that tells *any decided instance* from *the latest*.
//!
//! **D2 is green through the endpoint, by construction.** No integration
//! fixture decides an even number of documents, so the lower middle and the mean
//! agree on every one of them. The rule is the domain's, and the unit test that
//! reddened is where it is held. A fixture of four documents here would test the
//! same arithmetic a second time, through a database.
//!
//! **M12 and M14 redden more than one test each, and that is recorded rather
//! than tuned away.** Reversing the window empties every fixture that decided
//! something recently, and a permission added to one read of the summary fails
//! every caller the older sections deliberately gave the dashboard grant alone.
//!
//! **The first run of M3 came back with no result**, `LNK1104` on the previous
//! run's locked test binary. That is not a green and was not recorded as one;
//! the runner now retries a locked link, and M3 was run again and reddened as
//! tabled. A mutated statement is not in `.sqlx`, so the fourteen `M` mutations
//! compiled against a live schema. Every anchor had to occur exactly once in its
//! file or the runner refused it.
//!
//! **Not run, and why.** `id DESC` as the tie-break under `started_at DESC` is
//! not mutated on its own: UUIDv7 ids are time-ordered, so it breaks a tie the
//! same way `started_at` would have, and no fixture here has two rounds of one
//! document starting in the same microsecond.
//!
//! [#461]: https://github.com/sujanto-gaws/kelir/issues/461
//!
//! [#106]: https://github.com/sujanto-gaws/kelir/issues/106
//! [#121]: https://github.com/sujanto-gaws/kelir/issues/121
//! [#218]: https://github.com/sujanto-gaws/kelir/issues/218
//! [#431]: https://github.com/sujanto-gaws/kelir/issues/431

mod common;

use axum::http::{Method, StatusCode};
use chrono::{DateTime, Duration, Utc};
use common::{fixtures, TestApp};
use serde_json::{json, Value};
use uuid::Uuid;

const SUMMARY: &str = "/api/v1/dashboard/summary";
const TASKS: &str = "/api/v1/tasks";

fn id_of(value: &Value) -> Uuid {
    value["id"]
        .as_str()
        .expect("an id")
        .parse()
        .expect("a uuid")
}

/// A workflow whose one task is offered to the role named.
fn workflow_for(key: &str, role_code: &str) -> Value {
    json!({
        "workflowKey": key,
        "version": "1.0.0",
        "name": "Standard approval",
        "initialState": "MANAGER_APPROVAL",
        "states": [
            { "code": "MANAGER_APPROVAL", "name": "Manager approval",
              "mapsToDocumentStatus": "PENDING_APPROVAL",
              "task": { "taskDefinitionKey": "manager_approval",
                        "taskName": "Approve the request",
                        "assignment": { "assigneeType": "ROLE", "roleCode": role_code } } },
            { "code": "COMPLETED", "name": "Completed", "mapsToDocumentStatus": "COMPLETED",
              "isFinal": true },
            { "code": "REJECTED", "name": "Rejected", "mapsToDocumentStatus": "REJECTED",
              "isFinal": true }
        ],
        "transitions": [
            { "from": "MANAGER_APPROVAL", "to": "COMPLETED", "action": "APPROVE",
              "allowedBy": format!("ROLE:{role_code}") },
            { "from": "MANAGER_APPROVAL", "to": "REJECTED", "action": "REJECT",
              "allowedBy": format!("ROLE:{role_code}") }
        ]
    })
}

async fn publish_workflow(app: &TestApp, token: &str, key: &str, role_code: &str) -> Uuid {
    publish_workflow_definition(app, token, key, workflow_for(key, role_code)).await
}

/// The same, for the two tests that need an assignment `workflow_for` does not
/// write — a department-scoped role, and one named person.
async fn publish_workflow_definition(
    app: &TestApp,
    token: &str,
    key: &str,
    definition: Value,
) -> Uuid {
    let created = app
        .post(
            "/api/v1/workflow/definitions",
            Some(token),
            json!({
                "workflowKey": key,
                "name": "Standard approval",
                "definition": definition,
            }),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);

    let id = id_of(&created.body["data"]);

    let publication = app
        .post(
            &format!("/api/v1/workflow/definitions/{id}/publication"),
            Some(token),
            json!({}),
        )
        .await;
    assert_eq!(publication.status, StatusCode::OK, "{}", publication.body);

    id
}

async fn published_form(app: &TestApp, token: &str, key: &str) -> Uuid {
    let created = app
        .post(
            "/api/v1/rad/forms",
            Some(token),
            json!({
                "formKey": key,
                "title": "Purchase requisition",
                "definition": {
                    "formId": key,
                    "version": "2.0.1",
                    "title": "Purchase requisition",
                    "components": [{
                        "id": "amount-field", "role": "data", "type": "number",
                        "key": "amount", "label": "Amount",
                        "validation": { "type": "number", "minimum": 0 }
                    }]
                },
            }),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);

    let id = id_of(&created.body["data"]);
    let published = app
        .post(
            &format!("/api/v1/rad/forms/{id}/publish"),
            Some(token),
            json!({}),
        )
        .await;
    assert_eq!(published.status, StatusCode::OK, "{}", published.body);

    id
}

/// A document type bound to `workflow`, with a numbering template of its own.
///
/// The template carries the type code because
/// `uq_documents_tenant_id_document_number` is tenant-wide while a numbering
/// bucket is per type — the finding `workflow_engine.rs` and `task_inbox.rs`
/// both record.
async fn document_type(app: &TestApp, token: &str, code: &str, workflow: Uuid) -> Uuid {
    let form = published_form(app, token, &code.to_lowercase().replace('_', "-")).await;

    let created = app
        .post(
            "/api/v1/document-types",
            Some(token),
            json!({
                "typeCode": code,
                "name": code,
                "formId": form,
                "workflows": [{ "workflowDefinitionId": workflow }],
            }),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);

    let type_id = id_of(&created.body["data"]);

    let rule = app
        .put(
            &format!("/api/v1/document-types/{type_id}/numbering-rule"),
            Some(token),
            json!({
                "ruleTemplate": format!("{code}-{{year}}-{{sequence}}"),
                "sequenceScope": "YEAR",
                "gapPolicy": "GAPLESS",
            }),
        )
        .await;
    assert_eq!(rule.status, StatusCode::OK, "{}", rule.body);

    type_id
}

/// Creates a document and leaves it in `DRAFT`.
async fn draft_document(app: &TestApp, token: &str, type_id: Uuid, title: &str) -> Uuid {
    let created = app
        .post(
            "/api/v1/documents",
            Some(token),
            json!({
                "documentTypeId": type_id,
                "title": title,
                "formData": { "amount": 1_000 },
            }),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);

    id_of(&created.body["data"])
}

/// Creates a document and submits it, so it leaves `DRAFT` and raises a task.
async fn submitted_document(app: &TestApp, token: &str, type_id: Uuid, title: &str) -> Uuid {
    let id = draft_document(app, token, type_id, title).await;

    let submitted = app
        .send(
            Method::POST,
            &format!("/api/v1/documents/{id}/submission"),
            Some(token),
            None,
        )
        .await;
    assert_eq!(submitted.status, StatusCode::OK, "{}", submitted.body);

    id
}

/// What a dashboard caller holds, before any test adds to it.
const HOLDER_PERMISSIONS: &[&str] = &[
    "reporting:dashboard:read",
    "workflow:task:read",
    "workflow:instance:read",
    "document:read",
    "document:create",
    // So a fixture author can send their own draft, which is what
    // `a_submitted_document_has_left_the_drafts_count` needs in order to move a
    // row out of the count rather than merely never add it.
    "document:submit",
    // And so an approver can finish one, which is what
    // `a_finished_task_has_left_the_waiting_count` needs for the same reason —
    // see the note on that test.
    "workflow:task:execute",
];

/// A role holding what the dashboard and the surfaces behind it need, and a
/// user holding that role.
async fn holder(app: &TestApp, role_code: &str, username: &str) -> String {
    holder_party(app, role_code, username, &[]).await.1
}

/// The same, with the holder's **id** as well as their token, and room for a
/// permission the ordinary caller has no use for.
///
/// The delegation test needs the id because a `USER` assignment names a person
/// in the workflow definition itself, and it needs `identity:delegation:create`
/// because a window is opened by the person handing their work over.
async fn holder_party(
    app: &TestApp,
    role_code: &str,
    username: &str,
    extra: &[&str],
) -> (Uuid, String) {
    let mut permissions = HOLDER_PERMISSIONS.to_vec();
    permissions.extend_from_slice(extra);

    let role = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        role_code,
        &permissions,
    )
    .await;

    let id = fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        username,
        &format!("{username}@example.test"),
        common::ADMIN_PASSWORD,
        &[role],
    )
    .await;

    (id, app.sign_in(username, common::ADMIN_PASSWORD).await)
}

async fn user_with_roles(app: &TestApp, username: &str, roles: &[Uuid]) -> String {
    fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        username,
        &format!("{username}@example.test"),
        common::ADMIN_PASSWORD,
        roles,
    )
    .await;

    app.sign_in(username, common::ADMIN_PASSWORD).await
}

async fn summary_of(app: &TestApp, token: &str) -> Value {
    let response = app.get(SUMMARY, Some(token)).await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.body);

    response.body["data"].clone()
}

/// The ids of the tasks the widget listed, in the order it listed them.
fn pending_ids(summary: &Value) -> Vec<String> {
    summary["pendingTasks"]
        .as_array()
        .unwrap_or_else(|| panic!("the summary carries a pendingTasks array: {summary}"))
        .iter()
        .map(|task| {
            task["id"]
                .as_str()
                .unwrap_or_else(|| panic!("a pending task has an id: {task}"))
                .to_owned()
        })
        .collect()
}

/// The ids of the tasks the **inbox** listed, in the order it listed them.
///
/// The widget's rows are asserted against these rather than against a fixture's
/// expectations, which is what makes the tests below divergence tests rather
/// than presence tests ([#432] AC2): a task one surface lists and the other
/// does not is the defect, whichever way round it falls.
async fn inbox_ids(app: &TestApp, token: &str) -> Vec<String> {
    let response = app.get(TASKS, Some(token)).await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.body);

    response.body["data"]
        .as_array()
        .expect("the inbox is a page")
        .iter()
        .map(|task| task["id"].as_str().expect("an id").to_owned())
        .collect()
}

/// A workflow whose one task is offered to a role **within one department**.
fn department_workflow(key: &str, role_code: &str, department_code: &str) -> Value {
    let mut definition = workflow_for(key, role_code);

    definition["states"][0]["task"]["assignment"] = json!({
        "assigneeType": "DEPARTMENT_ROLE",
        "roleCode": role_code,
        "departmentScope": department_code,
    });

    definition
}

/// A workflow whose one task is assigned to one named person.
///
/// A window redirects work that resolves to somebody, and a role task has no
/// assignee to redirect — `workflow::service::assignment`'s header says why — so
/// the delegated case has to be a `USER` assignment.
fn user_workflow(key: &str, assignee: Uuid) -> Value {
    let mut definition = workflow_for(key, "UNUSED");

    definition["states"][0]["task"]["assignment"] =
        json!({ "assigneeType": "USER", "userId": assignee.to_string() });

    for transition in definition["transitions"]
        .as_array_mut()
        .expect("the transitions")
    {
        transition["allowedBy"] = json!({ "assigneeType": "USER", "userId": assignee.to_string() });
    }

    definition
}

/// A department, by code.
async fn department(app: &TestApp, code: &str, name: &str) -> Uuid {
    let id = Uuid::now_v7();

    sqlx::query(
        "INSERT INTO departments (id, tenant_id, department_code, name) VALUES ($1, $2, $3, $4)",
    )
    .bind(id)
    .bind(fixtures::SYSTEM_TENANT_ID)
    .bind(code)
    .bind(name)
    .execute(&app.pool)
    .await
    .expect("insert the department");

    id
}

/// Scopes a user's grant to one department.
///
/// `user_roles.department_id` is the optional department-scoped grant `0002`
/// created and the column `DEPARTMENT_ROLE` resolves against.
async fn scope_grant_to(app: &TestApp, username: &str, department: Uuid) {
    let updated = sqlx::query(
        "UPDATE user_roles SET department_id = $1 \
         WHERE user_id = (SELECT id FROM users WHERE username = $2)",
    )
    .bind(department)
    .bind(username)
    .execute(&app.pool)
    .await
    .expect("scope the grant");

    assert_eq!(
        updated.rows_affected(),
        1,
        "the fixture scoped no grant, so the department clause would be untested"
    );
}

// ---------------------------------------------------------------------------
// AC2 — the permission, and the only one
// ---------------------------------------------------------------------------

/// **`reporting:dashboard:read` gates the surface.**
///
/// The caller below holds `document:read` and `workflow:task:read` — every
/// permission over the records the summary counts — and is still refused,
/// because the dashboard is a surface of its own rather than a projection of
/// those two.
#[tokio::test]
async fn the_summary_refuses_a_caller_without_the_grant() {
    let app = TestApp::spawn().await;

    let role = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "RD-NO-DASH",
        &["document:read", "workflow:task:read"],
    )
    .await;
    let token = user_with_roles(&app, "rd.nodash", &[role]).await;

    let response = app.get(SUMMARY, Some(&token)).await;

    assert_eq!(
        response.status,
        StatusCode::FORBIDDEN,
        "a caller without reporting:dashboard:read was served the dashboard: {}",
        response.body
    );
}

/// **And it is the only one it asks for** (ADR-0039, and the D-45 / D-47 shape
/// one module over).
///
/// The caller below holds `reporting:dashboard:read` and **nothing else** — no
/// `document:read`, no `workflow:task:read` — and is served. That is the
/// invariant the module rests on stated as a test: every number on the
/// dashboard is the caller's own work, so there is no second surface's
/// permission to ask for. A summary that required the underlying reads would
/// refuse this caller a count of their own drafts, which is
/// `0041_activity_read_dropped.sql`'s mistake arriving from the other
/// direction.
#[tokio::test]
async fn the_summary_asks_for_no_permission_but_its_own() {
    let app = TestApp::spawn().await;

    let role = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "RD-ONLY-DASH",
        &["reporting:dashboard:read"],
    )
    .await;
    let token = user_with_roles(&app, "rd.onlydash", &[role]).await;

    let response = app.get(SUMMARY, Some(&token)).await;

    assert_eq!(
        response.status,
        StatusCode::OK,
        "a caller holding only reporting:dashboard:read was refused: {}",
        response.body
    );
    assert_eq!(response.body["data"]["tasksWaiting"], 0);
    assert_eq!(response.body["data"]["draftDocuments"], 0);
    // FR-RPT-004 (#447) is served to this caller too, and a caller who has
    // raised nothing gets the whole axis at zero rather than an empty array.
    assert_eq!(
        by_status(&response.body["data"]),
        axis_with(&[]),
        "a caller with nothing raised did not get ten zeros: {}",
        response.body
    );
}

#[tokio::test]
async fn the_summary_needs_a_session() {
    let app = TestApp::spawn().await;

    let response = app.get(SUMMARY, None).await;

    assert_eq!(
        response.status,
        StatusCode::UNAUTHORIZED,
        "{}",
        response.body
    );
}

// ---------------------------------------------------------------------------
// AC3 — the viewer's scoping, asserted on rows
// ---------------------------------------------------------------------------

/// **A caller's waiting work is their own, and nobody else's.**
///
/// Two roles, two holders, two document types, two submitted documents — so
/// there are two open tasks in the tenant and each holder must see exactly one.
/// A single subject would read `1` under a correct rule and under no rule at
/// all.
#[tokio::test]
async fn the_summary_counts_the_callers_own_waiting_work_and_nobody_elses() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let finance = holder(&app, "RD-FINANCE", "rd.finance").await;
    let legal = holder(&app, "RD-LEGAL", "rd.legal").await;

    let finance_workflow = publish_workflow(&app, &token, "rd_finance", "RD-FINANCE").await;
    let legal_workflow = publish_workflow(&app, &token, "rd_legal", "RD-LEGAL").await;

    let finance_type = document_type(&app, &token, "RD_FIN", finance_workflow).await;
    let legal_type = document_type(&app, &token, "RD_LEG", legal_workflow).await;

    submitted_document(&app, &token, finance_type, "A finance request").await;
    submitted_document(&app, &token, legal_type, "A legal request").await;

    assert_eq!(
        summary_of(&app, &finance).await["tasksWaiting"],
        1,
        "the finance approver's count included a task that is not theirs"
    );
    assert_eq!(
        summary_of(&app, &legal).await["tasksWaiting"],
        1,
        "the legal approver's count included a task that is not theirs"
    );
}

/// **The number on the dashboard is the number in the inbox**, because there is
/// one of them.
///
/// This is the test that catches the failure this module's shape exists to
/// prevent. `count_waiting` goes through `workflow::repository::inbox`'s own
/// statement; a dashboard that counted its own way would be a second answer to
/// *whose task is this*, and the two would drift exactly as
/// [#279](https://github.com/sujanto-gaws/kelir/issues/279) records them
/// drifting inside that file — an inbox that said 23 and ended at 19.
///
/// Asserted against `meta.total` **and** the page length, so a count that
/// agreed with a wrong total would still be caught.
#[tokio::test]
async fn the_summary_and_the_inbox_report_the_same_number() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let finance = holder(&app, "RD-SAME", "rd.same").await;
    let other = holder(&app, "RD-OTHER", "rd.other").await;

    let workflow = publish_workflow(&app, &token, "rd_same", "RD-SAME").await;
    let other_workflow = publish_workflow(&app, &token, "rd_other", "RD-OTHER").await;

    let type_id = document_type(&app, &token, "RD_SAME", workflow).await;
    let other_type = document_type(&app, &token, "RD_OTH", other_workflow).await;

    submitted_document(&app, &token, type_id, "One").await;
    submitted_document(&app, &token, type_id, "Two").await;
    // Somebody else's, so a rule that counted everything would disagree with a
    // page that does not.
    submitted_document(&app, &token, other_type, "Not theirs").await;

    let inbox = app.get(TASKS, Some(&finance)).await;
    assert_eq!(inbox.status, StatusCode::OK, "{}", inbox.body);

    let rows = inbox.body["data"].as_array().expect("a page").len();
    let total = &inbox.body["meta"]["total"];
    let waiting = summary_of(&app, &finance).await["tasksWaiting"].clone();

    assert_eq!(rows, 2, "the fixture did not raise the tasks it meant to");
    assert_eq!(
        &waiting, total,
        "the dashboard says {waiting} and the inbox's total says {total} — \
         two answers to whose task is this"
    );
    assert_eq!(
        waiting.as_i64(),
        Some(rows as i64),
        "the dashboard's count and the inbox's page disagree"
    );

    // And the other holder's, so the assertion above is not passing because
    // every number in this test happens to be two.
    assert_eq!(summary_of(&app, &other).await["tasksWaiting"], 1);
}

/// **A finished task has left the waiting count.**
///
/// # This test exists because a mutation survived
///
/// `count_waiting` reads `InboxScope::Open`, and changing it to
/// `InboxScope::All` **passed every other test in this file** on 2026-09-12.
/// The reason was the fixtures: not one of them had a task that had been
/// *finished*, so `open` and `all` were the same set and the scope was not
/// under test at all. Every assertion about the waiting count was really an
/// assertion about *whose* task it is, and the *is it still waiting* half was
/// uncovered.
///
/// **So the mutation is the author of this test**, which is the whole of what
/// coding standard §2.9 asks a mutation run to be for — a survivor is a gap
/// found, not a run that went well. The task below is finished through the API
/// rather than by an `UPDATE`, so the row reaches the state the product
/// actually produces.
#[tokio::test]
async fn a_finished_task_has_left_the_waiting_count() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let approver = holder(&app, "RD-DONE", "rd.done").await;
    let workflow = publish_workflow(&app, &token, "rd_done", "RD-DONE").await;
    let type_id = document_type(&app, &token, "RD_DONE", workflow).await;

    let finished = submitted_document(&app, &token, type_id, "The one they decide").await;
    submitted_document(&app, &token, type_id, "The one still waiting").await;

    assert_eq!(
        summary_of(&app, &approver).await["tasksWaiting"],
        2,
        "the fixture did not raise the two tasks it meant to"
    );

    let task: Uuid = sqlx::query_scalar(
        "SELECT id FROM workflow_tasks WHERE document_id = $1 \
         AND status IN ('CREATED','ASSIGNED','IN_PROGRESS')",
    )
    .bind(finished)
    .fetch_one(&app.pool)
    .await
    .expect("read the open task");

    let decided = app
        .post(
            &format!("/api/v1/workflow/tasks/{task}/decision"),
            Some(&approver),
            json!({ "action": "APPROVE" }),
        )
        .await;
    assert_eq!(decided.status, StatusCode::OK, "{}", decided.body);

    let summary = summary_of(&app, &approver).await;

    assert_eq!(
        summary["tasksWaiting"], 1,
        "a task this caller has already decided was still counted as waiting"
    );
    // **And it has left the rows too.** The count and the list come from one
    // statement now (#432 AC5), so asserting only the number would leave the
    // scope covered on one of the two things that statement answers.
    assert_eq!(
        pending_ids(&summary).len(),
        1,
        "a decided task was still listed on the dashboard: {summary}"
    );
}

/// **Overdue counts only what is late**, and it is a subset of what is waiting.
///
/// The due date is set on one of the two tasks directly, because what is under
/// test is the count rather than the engine's own due-date derivation — which
/// `workflow_due_dates.rs` owns.
#[tokio::test]
async fn overdue_counts_only_what_is_late() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let approver = holder(&app, "RD-LATE", "rd.late").await;
    let workflow = publish_workflow(&app, &token, "rd_late", "RD-LATE").await;
    let type_id = document_type(&app, &token, "RD_LATE", workflow).await;

    let late = submitted_document(&app, &token, type_id, "The late one").await;
    submitted_document(&app, &token, type_id, "The timely one").await;

    sqlx::query(
        "UPDATE workflow_tasks SET due_at = now() - interval '1 day' WHERE document_id = $1",
    )
    .bind(late)
    .execute(&app.pool)
    .await
    .expect("set a due date in the past");

    let summary = summary_of(&app, &approver).await;

    assert_eq!(
        summary["tasksWaiting"], 2,
        "a task that is late is still waiting: {summary}"
    );
    assert_eq!(
        summary["tasksOverdue"], 1,
        "the undated task was reported as late, or the late one was not: {summary}"
    );
}

// ---------------------------------------------------------------------------
// FR-RPT-002 — the pending-task widget ([#432])
// ---------------------------------------------------------------------------

/// **The widget lists the rows the inbox lists.**
///
/// # A divergence test, not a presence test
///
/// [#432] AC2 asks for exactly this and says why: *a task the inbox lists and
/// the widget does not, or the reverse, is the defect.* A test that merely
/// asserted the widget returned some rows would pass for a widget that had
/// grown a visibility rule of its own — which is the one failure the whole
/// shape of this module exists to prevent, and the one
/// [#279](https://github.com/sujanto-gaws/kelir/issues/279) records the cost of.
///
/// So the expected value is **what the inbox answered**, read from the inbox in
/// the same test, rather than a list the fixture wrote down. The second holder
/// is here for the reason coding standard §2.9 gives: one subject cannot tell
/// *scoped to this caller* from *not scoped at all*, because a widget that
/// listed the tenant's tasks would read identically.
#[tokio::test]
async fn the_widget_lists_the_rows_the_inbox_lists() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let theirs = holder(&app, "RD-W-SAME", "rd.w.same").await;
    let other = holder(&app, "RD-W-OTHER", "rd.w.other").await;

    let workflow = publish_workflow(&app, &token, "rd_w_same", "RD-W-SAME").await;
    let other_workflow = publish_workflow(&app, &token, "rd_w_other", "RD-W-OTHER").await;
    let type_id = document_type(&app, &token, "RD_W_SAME", workflow).await;
    let other_type = document_type(&app, &token, "RD_W_OTH", other_workflow).await;

    submitted_document(&app, &token, type_id, "One").await;
    submitted_document(&app, &token, type_id, "Two").await;
    submitted_document(&app, &token, other_type, "Not theirs").await;

    let inbox = inbox_ids(&app, &theirs).await;
    let summary = summary_of(&app, &theirs).await;

    assert_eq!(
        inbox.len(),
        2,
        "the fixture did not raise the tasks it meant to"
    );
    assert_eq!(
        pending_ids(&summary),
        inbox,
        "the widget and the inbox disagree about which tasks are waiting — \
         two answers to whose task is this: {summary}"
    );

    // The other holder's, so the assertion above is not passing because every
    // row in this test belongs to everybody.
    let other_inbox = inbox_ids(&app, &other).await;
    let other_summary = summary_of(&app, &other).await;

    assert_eq!(other_inbox.len(), 1);
    assert_eq!(
        pending_ids(&other_summary),
        other_inbox,
        "the second holder's widget does not match their own inbox: {other_summary}"
    );
    assert!(
        pending_ids(&summary)
            .iter()
            .all(|id| !other_inbox.contains(id)),
        "one person's waiting work appeared on another person's dashboard"
    );
}

/// **The card shows the top of the queue, and the count says how deep it is.**
///
/// Six tasks, five rows, and `tasksWaiting` reads six ([#432] AC1, AC5). The
/// count is not the length of the list and must not be derived from it — which
/// is what makes *3 of 12* sayable, and what this fixture is sized to catch: a
/// widget whose count came from `pendingTasks.length` would read five here and
/// be wrong by exactly the number of tasks the person cannot see.
///
/// The order is the inbox's own, so the rows are a **prefix** of it rather than
/// a selection out of it.
#[tokio::test]
async fn the_card_shows_the_top_of_the_queue_and_counts_the_whole_of_it() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let approver = holder(&app, "RD-W-DEEP", "rd.w.deep").await;
    let workflow = publish_workflow(&app, &token, "rd_w_deep", "RD-W-DEEP").await;
    let type_id = document_type(&app, &token, "RD_W_DEEP", workflow).await;

    for n in 1..=6 {
        submitted_document(&app, &token, type_id, &format!("Requisition {n}")).await;
    }

    let summary = summary_of(&app, &approver).await;
    let listed = pending_ids(&summary);

    assert_eq!(
        summary["tasksWaiting"], 6,
        "the count describes the queue, not the card: {summary}"
    );
    assert_eq!(
        listed.len(),
        5,
        "the card carries reporting::PENDING_TASKS_SHOWN rows: {summary}"
    );

    let inbox = inbox_ids(&app, &approver).await;
    assert_eq!(inbox.len(), 6, "the fixture did not raise six tasks");
    assert_eq!(
        listed,
        inbox[..5].to_vec(),
        "the widget is not the top of the inbox — it has an order of its own"
    );
}

/// **Nothing waiting is an empty list beside a zero, and never a missing
/// field** ([#432] AC4).
///
/// The screen has to be able to say *nothing is waiting for you* in words,
/// because an empty card is indistinguishable from one that failed to load.
/// What the contract owes it is the difference between *no rows* and *no
/// answer*, and `pendingTasks` is an array either way.
#[tokio::test]
async fn a_viewer_with_nothing_waiting_gets_an_empty_list_and_a_zero() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let idle = holder(&app, "RD-W-IDLE", "rd.w.idle").await;
    let busy = holder(&app, "RD-W-BUSY", "rd.w.busy").await;

    // Somebody else's task exists, so this asserts *empty for this caller*
    // rather than *empty because the tenant has no tasks at all*.
    let workflow = publish_workflow(&app, &token, "rd_w_busy", "RD-W-BUSY").await;
    let type_id = document_type(&app, &token, "RD_W_BUSY", workflow).await;
    submitted_document(&app, &token, type_id, "Somebody else's").await;

    let summary = summary_of(&app, &idle).await;

    assert!(
        summary["pendingTasks"].is_array(),
        "pendingTasks is an array even when it is empty: {summary}"
    );
    assert_eq!(pending_ids(&summary), Vec::<String>::new());
    assert_eq!(summary["tasksWaiting"], 0);

    assert_eq!(
        pending_ids(&summary_of(&app, &busy).await).len(),
        1,
        "the fixture's own task was not raised, so the empty above means nothing"
    );
}

/// **A department-scoped task is on that department's widget and no other**
/// ([#432] AC6).
///
/// # One of the two cases that has already cost this project a fix
///
/// The `candidate_department_id` clause arrived with
/// [#225](https://github.com/sujanto-gaws/kelir/issues/225), which closed the
/// half of `DEPARTMENT_ROLE` that was resolved, stored and then read by
/// nothing. It is the clause a fresh implementation of *whose task is this*
/// leaves out first, and the widget inherits it only because it inherits the
/// whole statement.
///
/// Both approvers hold the **same role** and differ only in the department
/// their grant names, which is the fixture that makes the clause observable: a
/// predicate that dropped it lists Finance's task on Procurement's dashboard,
/// and every other assertion in this file would still pass.
#[tokio::test]
async fn a_department_scoped_task_reaches_only_that_departments_widget() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let finance = department(&app, "RD-DEPT-FIN", "Finance").await;
    let procurement = department(&app, "RD-DEPT-PROC", "Procurement").await;

    let role = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "RD-W-DEPT",
        HOLDER_PERMISSIONS,
    )
    .await;

    let insider = user_with_roles(&app, "rd.w.fin", &[role]).await;
    scope_grant_to(&app, "rd.w.fin", finance).await;
    let outsider = user_with_roles(&app, "rd.w.proc", &[role]).await;
    scope_grant_to(&app, "rd.w.proc", procurement).await;

    let workflow = publish_workflow_definition(
        &app,
        &token,
        "rd_w_dept",
        department_workflow("rd_w_dept", "RD-W-DEPT", "RD-DEPT-FIN"),
    )
    .await;
    let type_id = document_type(&app, &token, "RD_W_DEPT", workflow).await;

    submitted_document(&app, &token, type_id, "Finance's requisition").await;

    let theirs = summary_of(&app, &insider).await;
    let not_theirs = summary_of(&app, &outsider).await;

    assert_eq!(
        pending_ids(&theirs),
        inbox_ids(&app, &insider).await,
        "the department's own approver sees different work on the two surfaces: {theirs}"
    );
    assert_eq!(
        pending_ids(&theirs).len(),
        1,
        "the department's own approver was not offered their task: {theirs}"
    );
    assert_eq!(
        pending_ids(&not_theirs),
        Vec::<String>::new(),
        "Procurement's approver was offered Finance's task on the dashboard: {not_theirs}"
    );
    assert_eq!(
        not_theirs["tasksWaiting"], 0,
        "the count crossed the department boundary even though the rows did not: {not_theirs}"
    );
}

/// **A delegated task is on the delegate's widget, and off the delegator's**
/// ([#432] AC6).
///
/// # The other case, and the one whose failure is silent
///
/// [#234](https://github.com/sujanto-gaws/kelir/pull/234) built delegation end
/// to end, and a widget that forgot it **shows a delegate an empty dashboard
/// beside a full inbox** — a person told there is nothing waiting for them
/// while there is. That is worse than a wrong number, because nobody reports
/// the absence of work.
///
/// The window is opened through the API by the delegator in their own name,
/// which is the only way one can be opened, so the row reaches the state the
/// product actually produces rather than one an `UPDATE` invented.
#[tokio::test]
async fn a_delegated_task_is_on_the_delegates_widget_and_not_the_delegators() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let (delegator_id, delegator) = holder_party(
        &app,
        "RD-W-DELEGATOR",
        "rd.w.ani",
        &["identity:delegation:create"],
    )
    .await;
    let (delegate_id, delegate) = holder_party(&app, "RD-W-DELEGATE", "rd.w.budi", &[]).await;

    let opened = app
        .post(
            "/api/v1/identity/delegations",
            Some(&delegator),
            json!({
                "delegateUserId": delegate_id,
                "startsAt": (Utc::now() - Duration::hours(1)).to_rfc3339(),
                "endsAt": (Utc::now() + Duration::hours(1)).to_rfc3339(),
            }),
        )
        .await;
    assert_eq!(opened.status, StatusCode::CREATED, "{}", opened.body);

    let workflow = publish_workflow_definition(
        &app,
        &token,
        "rd_w_deleg",
        user_workflow("rd_w_deleg", delegator_id),
    )
    .await;
    let type_id = document_type(&app, &token, "RD_W_DELEG", workflow).await;

    submitted_document(&app, &token, type_id, "Approved on Ani's behalf").await;

    let theirs = summary_of(&app, &delegate).await;
    let handed_over = summary_of(&app, &delegator).await;

    assert_eq!(
        pending_ids(&theirs),
        inbox_ids(&app, &delegate).await,
        "the delegate's dashboard and inbox disagree: {theirs}"
    );
    assert_eq!(
        pending_ids(&theirs).len(),
        1,
        "the delegate has an empty dashboard beside a full inbox — the widget \
         forgot delegation: {theirs}"
    );
    assert_eq!(
        theirs["pendingTasks"][0]["delegatedFromUserId"],
        json!(delegator_id.to_string()),
        "the row does not say whose approval it is, so the card cannot write \
         \"on Ani's behalf\": {theirs}"
    );
    assert_eq!(
        pending_ids(&handed_over),
        Vec::<String>::new(),
        "the task is still on the dashboard of the person who handed it over: {handed_over}"
    );
}

// ---------------------------------------------------------------------------
// The drafts half — the caller's own, in their own tenant, still unsent
// ---------------------------------------------------------------------------

/// **Drafts are the caller's own, not the tenant's.**
///
/// Two authors, two drafts. A predicate that dropped `created_by` reads `2` for
/// both, which is what this fixture exists to make visible.
#[tokio::test]
async fn drafts_are_the_callers_own_not_the_tenants() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let workflow = publish_workflow(&app, &token, "rd_drafts", "RD-DRAFTS").await;
    let type_id = document_type(&app, &token, "RD_DRAFT", workflow).await;

    let ani = holder(&app, "RD-DRAFTS", "rd.ani").await;
    let budi = holder(&app, "RD-DRAFTS-2", "rd.budi").await;

    draft_document(&app, &ani, type_id, "Ani's draft").await;
    draft_document(&app, &budi, type_id, "Budi's draft").await;
    draft_document(&app, &budi, type_id, "Budi's second draft").await;

    assert_eq!(
        summary_of(&app, &ani).await["draftDocuments"],
        1,
        "Ani's dashboard counted somebody else's draft"
    );
    assert_eq!(
        summary_of(&app, &budi).await["draftDocuments"],
        2,
        "Budi's dashboard did not count both of his own"
    );
}

/// **A submitted document has left the drafts count.**
///
/// *What you have not sent yet* is the question, and a document in the middle
/// of an approval is not it.
#[tokio::test]
async fn a_submitted_document_has_left_the_drafts_count() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let workflow = publish_workflow(&app, &token, "rd_sent", "RD-SENT").await;
    let type_id = document_type(&app, &token, "RD_SENT", workflow).await;

    let author = holder(&app, "RD-SENT", "rd.author").await;

    draft_document(&app, &author, type_id, "Still a draft").await;
    submitted_document(&app, &author, type_id, "Already sent").await;

    assert_eq!(
        summary_of(&app, &author).await["draftDocuments"],
        1,
        "a submitted document was still counted as a draft"
    );
}

/// **A deleted draft is not counted.**
///
/// The soft delete is what the rest of the document surface honours, and a
/// count that ignored `deleted_at` would report work the author has thrown
/// away — and could never be made to go down.
#[tokio::test]
async fn a_deleted_draft_is_not_counted() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let workflow = publish_workflow(&app, &token, "rd_gone", "RD-GONE").await;
    let type_id = document_type(&app, &token, "RD_GONE", workflow).await;

    let author = holder(&app, "RD-GONE", "rd.gone").await;

    let kept = draft_document(&app, &author, type_id, "Kept").await;
    let discarded = draft_document(&app, &author, type_id, "Discarded").await;

    sqlx::query("UPDATE documents SET deleted_at = now() WHERE id = $1")
        .bind(discarded)
        .execute(&app.pool)
        .await
        .expect("soft-delete a draft");

    assert_eq!(
        summary_of(&app, &author).await["draftDocuments"],
        1,
        "a soft-deleted draft was counted"
    );

    // The kept one is still there, so the assertion above is not passing
    // because the count collapsed to zero.
    let still_there: i64 =
        sqlx::query_scalar("SELECT count(*) FROM documents WHERE id = $1 AND deleted_at IS NULL")
            .bind(kept)
            .fetch_one(&app.pool)
            .await
            .expect("read the kept draft");
    assert_eq!(still_there, 1);
}

/// **A second tenant's draft is not on this caller's dashboard.**
///
/// The row is written directly, with this caller as its author, because that is
/// the shape a dropped `tenant_id` predicate would expose and no API call can
/// produce it: the document endpoints stamp the caller's own tenant, so a test
/// that went through them could never tell a tenant-scoped count from an
/// unscoped one.
#[tokio::test]
async fn a_second_tenants_draft_is_not_on_this_callers_dashboard() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let workflow = publish_workflow(&app, &token, "rd_tenant", "RD-TENANT").await;
    let type_id = document_type(&app, &token, "RD_TEN", workflow).await;

    let author = holder(&app, "RD-TENANT", "rd.tenant").await;
    let author_id: Uuid =
        sqlx::query_scalar("SELECT id FROM users WHERE username = $1 AND tenant_id = $2")
            .bind("rd.tenant")
            .bind(fixtures::SYSTEM_TENANT_ID)
            .fetch_one(&app.pool)
            .await
            .expect("read the fixture user");

    draft_document(&app, &author, type_id, "Theirs, in their tenant").await;

    let other_tenant = fixtures::create_tenant(&app.pool, "RD-OTHER-TENANT", "Other tenant").await;

    sqlx::query(
        "INSERT INTO documents (id, tenant_id, created_by, document_ref, document_type_id, \
         title, status) VALUES ($1, $2, $3, $4, $5, $6, 'DRAFT')",
    )
    .bind(Uuid::now_v7())
    .bind(other_tenant)
    .bind(author_id)
    .bind("RD-OTHER-0001")
    .bind(type_id)
    .bind("A draft filed under another tenant")
    .execute(&app.pool)
    .await
    .expect("insert a second tenant's draft");

    assert_eq!(
        summary_of(&app, &author).await["draftDocuments"],
        1,
        "a draft in another tenant was counted on this caller's dashboard"
    );
}

// ---------------------------------------------------------------------------
// FR-RPT-003 — the recent-documents widget (#433)
// ---------------------------------------------------------------------------

/// The ids of the documents the widget listed, in the order it listed them.
fn recent_ids(summary: &Value) -> Vec<String> {
    summary["recentDocuments"]
        .as_array()
        .unwrap_or_else(|| panic!("the summary carries a recentDocuments array: {summary}"))
        .iter()
        .map(|document| {
            document["id"]
                .as_str()
                .unwrap_or_else(|| panic!("a recent document has an id: {document}"))
                .to_owned()
        })
        .collect()
}

/// Writes one activity event directly, for the cases no API call can produce.
///
/// **Three tests need a row the product will not write for them**: an event in
/// another tenant, one whose actor is the workflow engine rather than a person,
/// and an `Attachment.Downloaded`. The first two are the shapes a dropped
/// predicate would expose, and the third needs MinIO and ClamAV to arrive
/// through the attachment surface — a dependency this suite does not take for
/// one boolean.
#[allow(clippy::too_many_arguments)]
async fn record_touch(
    app: &TestApp,
    tenant_id: Uuid,
    document_id: Uuid,
    actor_user_id: Option<Uuid>,
    event_type: &str,
    at: chrono::DateTime<Utc>,
) {
    sqlx::query(
        "INSERT INTO activity_events (id, tenant_id, created_at, document_id, event_type, \
         event_category, actor_type, actor_user_id, action_summary) \
         VALUES ($1, $2, $3, $4, $5, 'DOCUMENT', $6, $7, 'written by a test')",
    )
    .bind(Uuid::now_v7())
    .bind(tenant_id)
    .bind(at)
    .bind(document_id)
    .bind(event_type)
    .bind(if actor_user_id.is_some() {
        "USER"
    } else {
        "WORKFLOW_ENGINE"
    })
    .bind(actor_user_id)
    .execute(&app.pool)
    .await
    .expect("insert an activity event");
}

async fn user_id_of(app: &TestApp, username: &str) -> Uuid {
    sqlx::query_scalar("SELECT id FROM users WHERE username = $1 AND tenant_id = $2")
        .bind(username)
        .bind(fixtures::SYSTEM_TENANT_ID)
        .fetch_one(&app.pool)
        .await
        .expect("read the fixture user")
}

/// **Raising a document is touching it, and the widget says so.**
///
/// The cheapest of the four verbs and the one every other test here rests on:
/// `draft_document` goes through `POST /documents`, which writes
/// `Document.Created` in the same transaction, so the row arrives in the widget
/// without the test writing an event of its own.
///
/// Two documents rather than one, because a widget that returned *everything in
/// the tenant* would also pass a single-row assertion.
#[tokio::test]
async fn the_widget_lists_the_documents_the_caller_raised() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let workflow = publish_workflow(&app, &token, "rd_recent", "RD-RECENT").await;
    let type_id = document_type(&app, &token, "RD_RCNT", workflow).await;

    let author = holder(&app, "RD-RECENT", "rd.recent").await;
    let stranger = holder(&app, "RD-RECENT-2", "rd.recent.other").await;

    let first = draft_document(&app, &author, type_id, "Mine, first").await;
    let second = draft_document(&app, &author, type_id, "Mine, second").await;
    let theirs = draft_document(&app, &stranger, type_id, "Not mine").await;

    let listed = recent_ids(&summary_of(&app, &author).await);

    assert_eq!(
        listed,
        vec![second.to_string(), first.to_string()],
        "the widget did not list the caller's own two documents newest-touch first"
    );
    assert!(
        !listed.contains(&theirs.to_string()),
        "a document only somebody else touched was on this caller's widget"
    );

    // And the other direction, so the assertion above is not passing because
    // the widget scoped to something else that happens to correlate.
    assert_eq!(
        recent_ids(&summary_of(&app, &stranger).await),
        vec![theirs.to_string()],
        "the stranger's own widget lost their own document"
    );
}

/// **Commenting on a document you did not raise makes it yours** ([#433] AC2).
///
/// This is the test that would fail for a widget built on
/// `documents.created_by`, which is the substitute an author reaching for
/// *recent documents* would try first. The commenter never touches
/// `POST /documents` for this row — somebody else raised it — and it still has
/// to appear, because *commented on* is one of the four verbs the definition
/// names.
///
/// **It is also the ordering case that matters**: the commenter's own document
/// is raised *after* the one they comment on, and the comment then re-floats the
/// other above it. A widget ordering by `documents.created_at` or by
/// `updated_at` passes the presence half of this test and fails here.
///
/// [#433]: https://github.com/sujanto-gaws/kelir/issues/433
#[tokio::test]
async fn commenting_on_somebody_elses_document_makes_it_recent_for_you() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let workflow = publish_workflow(&app, &token, "rd_cmt", "RD-CMT").await;
    let type_id = document_type(&app, &token, "RD_CMT", workflow).await;

    let owner = holder(&app, "RD-CMT-OWNER", "rd.cmt.owner").await;
    let commenter_role = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "RD-CMT-VOICE",
        &[
            "reporting:dashboard:read",
            "document:read",
            "document:create",
            "comment:create",
        ],
    )
    .await;
    let commenter = user_with_roles(&app, "rd.cmt.voice", &[commenter_role]).await;

    let theirs = draft_document(&app, &owner, type_id, "Raised by somebody else").await;
    // Raised *after* the document above, so ordering by the document's own
    // timestamps would put this one first.
    let mine = draft_document(&app, &commenter, type_id, "Raised by the commenter").await;

    let added = app
        .post(
            &format!("/api/v1/documents/{theirs}/comments"),
            Some(&commenter),
            json!({ "body": "is this the right supplier?" }),
        )
        .await;
    assert_eq!(added.status, StatusCode::OK, "{}", added.body);

    assert_eq!(
        recent_ids(&summary_of(&app, &commenter).await),
        vec![theirs.to_string(), mine.to_string()],
        "a comment did not make somebody else's document the commenter's most recent touch"
    );

    // The owner's own widget is unchanged by somebody else's comment: they
    // raised one document and touched one document.
    assert_eq!(
        recent_ids(&summary_of(&app, &owner).await),
        vec![theirs.to_string()],
        "the owner's widget changed when another person commented"
    );
}

/// **The newest touch decides the order, not the oldest.**
///
/// `max(created_at)` rather than `min`, and a document touched twice is one row
/// rather than two. Both halves are asserted, because a `GROUP BY` that took the
/// wrong aggregate would still return the right *set*.
#[tokio::test]
async fn the_newest_touch_decides_the_order() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let workflow = publish_workflow(&app, &token, "rd_order", "RD-ORDER").await;
    let type_id = document_type(&app, &token, "RD_ORD", workflow).await;

    let author = holder(&app, "RD-ORDER", "rd.order").await;
    let author_id = user_id_of(&app, "rd.order").await;

    let older = draft_document(&app, &author, type_id, "Touched again later").await;
    let newer = draft_document(&app, &author, type_id, "Touched once").await;

    // `newer` is ahead on its own creation event, so the fixture starts in the
    // order this test has to reverse.
    assert_eq!(
        recent_ids(&summary_of(&app, &author).await),
        vec![newer.to_string(), older.to_string()],
    );

    record_touch(
        &app,
        fixtures::SYSTEM_TENANT_ID,
        older,
        Some(author_id),
        "Document.Updated",
        Utc::now() + Duration::minutes(5),
    )
    .await;

    let listed = recent_ids(&summary_of(&app, &author).await);

    assert_eq!(
        listed,
        vec![older.to_string(), newer.to_string()],
        "a later touch did not re-float the document above one touched earlier"
    );
    assert_eq!(
        listed.len(),
        2,
        "a document touched twice was listed twice: {listed:?}"
    );
}

/// **A soft-deleted document is not recent** ([#433] AC6).
///
/// The activity events survive the soft delete — `activity_events` is
/// append-only and has no `deleted_at` of its own (`0033_activity.sql`) — so
/// this row is only excluded by the `deleted_at IS NULL` the statement inherits
/// from the document list. **That makes this the test that fails if the join
/// ever stops carrying it**, and the reason `Document.Deleted` is not in
/// `TOUCH_EVENT_TYPES`.
///
/// [#433]: https://github.com/sujanto-gaws/kelir/issues/433
#[tokio::test]
async fn a_soft_deleted_document_is_not_recent() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let workflow = publish_workflow(&app, &token, "rd_rgone", "RD-RGONE").await;
    let type_id = document_type(&app, &token, "RD_RGN", workflow).await;

    let author = holder(&app, "RD-RGONE", "rd.rgone").await;

    let kept = draft_document(&app, &author, type_id, "Kept").await;
    let discarded = draft_document(&app, &author, type_id, "Discarded").await;

    sqlx::query("UPDATE documents SET deleted_at = now() WHERE id = $1")
        .bind(discarded)
        .execute(&app.pool)
        .await
        .expect("soft-delete a document");

    // The events for the deleted document are still in the table, which is what
    // makes this an assertion about the join rather than about the fixture.
    let events_survive: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM activity_events WHERE document_id = $1 AND event_type = 'Document.Created'",
    )
    .bind(discarded)
    .fetch_one(&app.pool)
    .await
    .expect("read the deleted document's events");
    assert_eq!(
        events_survive, 1,
        "the fixture cannot prove the join excludes the row if the event went too"
    );

    assert_eq!(
        recent_ids(&summary_of(&app, &author).await),
        vec![kept.to_string()],
        "a soft-deleted document was on the recent-documents widget"
    );
}

/// **A second tenant's document is not recent here.**
///
/// Both rows are written directly — the document *and* its activity event —
/// with this caller as the actor, because that is the shape a dropped
/// `tenant_id` predicate would expose and no API call can produce it: the
/// endpoints stamp the caller's own tenant, so a test that went through them
/// could never tell a tenant-scoped read from an unscoped one ([#106]/[#121]).
///
/// **The join's `a.tenant_id = d.tenant_id` is the second half of this**, and it
/// is asserted by the event being written under the *other* tenant alongside
/// the document.
#[tokio::test]
async fn a_second_tenants_document_is_not_recent_here() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let workflow = publish_workflow(&app, &token, "rd_rten", "RD-RTEN").await;
    let type_id = document_type(&app, &token, "RD_RTN", workflow).await;

    let author = holder(&app, "RD-RTEN", "rd.rten").await;
    let author_id = user_id_of(&app, "rd.rten").await;

    let mine = draft_document(&app, &author, type_id, "Theirs, in their tenant").await;

    let other_tenant = fixtures::create_tenant(&app.pool, "RD-ROTHER-TENANT", "Other tenant").await;
    let elsewhere = Uuid::now_v7();

    sqlx::query(
        "INSERT INTO documents (id, tenant_id, created_by, document_ref, document_type_id, \
         title, status) VALUES ($1, $2, $3, $4, $5, $6, 'DRAFT')",
    )
    .bind(elsewhere)
    .bind(other_tenant)
    .bind(author_id)
    .bind("RD-ROTHER-0001")
    .bind(type_id)
    .bind("A document filed under another tenant")
    .execute(&app.pool)
    .await
    .expect("insert a second tenant's document");

    record_touch(
        &app,
        other_tenant,
        elsewhere,
        Some(author_id),
        "Document.Created",
        Utc::now() + Duration::minutes(5),
    )
    .await;

    assert_eq!(
        recent_ids(&summary_of(&app, &author).await),
        vec![mine.to_string()],
        "a document in another tenant was on this caller's widget"
    );
}

/// **Opening a file is not touching the document.**
///
/// `Attachment.Downloaded` is the one event type in the table that records
/// *looking* rather than doing, and it is the reason `TOUCH_EVENT_TYPES` is a
/// closed allow-list rather than "everything but". Under a deny-list this row
/// counts by default and the widget silently becomes *what you looked at* — a
/// different card, with a different privacy question, arrived at by nobody
/// deciding anything.
///
/// The event is written directly rather than through the attachment surface,
/// which would need MinIO and ClamAV for one boolean.
#[tokio::test]
async fn opening_a_file_does_not_make_a_document_recent() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let workflow = publish_workflow(&app, &token, "rd_read", "RD-READ").await;
    let type_id = document_type(&app, &token, "RD_READ", workflow).await;

    let author = holder(&app, "RD-READ", "rd.read").await;
    let author_id = user_id_of(&app, "rd.read").await;

    let reader_role = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "RD-READ-ONLY",
        &["reporting:dashboard:read", "document:read"],
    )
    .await;
    let reader = user_with_roles(&app, "rd.read.only", &[reader_role]).await;
    let reader_id = user_id_of(&app, "rd.read.only").await;

    let document = draft_document(&app, &author, type_id, "Somebody else's document").await;

    record_touch(
        &app,
        fixtures::SYSTEM_TENANT_ID,
        document,
        Some(reader_id),
        "Attachment.Downloaded",
        Utc::now(),
    )
    .await;

    assert!(
        recent_ids(&summary_of(&app, &reader).await).is_empty(),
        "downloading a file put a document on the reader's recent-documents widget"
    );

    // And the row is reachable — the same document is on its author's widget,
    // so the assertion above is not passing because the widget is broken.
    assert_eq!(
        recent_ids(&summary_of(&app, &author).await),
        vec![document.to_string()],
    );

    // Written under this caller and still not counted, which is what makes the
    // exclusion about the event type rather than about the actor.
    let downloads: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM activity_events WHERE actor_user_id = $1 AND event_type = 'Attachment.Downloaded'",
    )
    .bind(reader_id)
    .fetch_one(&app.pool)
    .await
    .expect("read the download event");
    assert_eq!(downloads, 1);
    assert_ne!(reader_id, author_id);
}

/// **What the workflow engine did is nobody's recent work.**
///
/// `activity_events.actor_user_id` is nullable and a system actor leaves it
/// null, so the predicate is an equality rather than anything looser: a person
/// scanning *what did I work on* is not looking for what a timer did, and a
/// `NULL` that matched would put every automated transition on every
/// dashboard in the tenant.
#[tokio::test]
async fn a_system_actor_is_nobodys_recent_work() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let workflow = publish_workflow(&app, &token, "rd_sys", "RD-SYS").await;
    let type_id = document_type(&app, &token, "RD_SYS", workflow).await;

    let author = holder(&app, "RD-SYS", "rd.sys").await;
    let onlooker_role = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "RD-SYS-ONLOOKER",
        &["reporting:dashboard:read", "document:read"],
    )
    .await;
    let onlooker = user_with_roles(&app, "rd.sys.onlooker", &[onlooker_role]).await;

    let document = draft_document(&app, &author, type_id, "Moved by the engine").await;

    record_touch(
        &app,
        fixtures::SYSTEM_TENANT_ID,
        document,
        None,
        "Document.StatusChanged",
        Utc::now() + Duration::minutes(5),
    )
    .await;

    assert!(
        recent_ids(&summary_of(&app, &onlooker).await).is_empty(),
        "a system-actor event reached a dashboard"
    );
    // The author still sees it, on their own `Document.Created` rather than on
    // the engine's event.
    assert_eq!(
        recent_ids(&summary_of(&app, &author).await),
        vec![document.to_string()],
    );
}

/// **The card shows five and does not grow.**
///
/// `RECENT_DOCUMENTS_SHOWN`. Six documents, five rows, and the one left out is
/// the oldest touch rather than an arbitrary one — which is the half a `LIMIT`
/// without an `ORDER BY` would get wrong while still returning five.
///
/// **There is no count beside the list**, unlike the pending-task widget: the
/// assertion is that `recentDocuments` is all the widget claims to carry, and
/// *how many documents have you ever touched* is not a question the card asks.
#[tokio::test]
async fn the_card_shows_five_and_drops_the_oldest_touch() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let workflow = publish_workflow(&app, &token, "rd_five", "RD-FIVE").await;
    let type_id = document_type(&app, &token, "RD_FIVE", workflow).await;

    let author = holder(&app, "RD-FIVE", "rd.five").await;

    let mut created = Vec::new();
    for index in 0..6 {
        created.push(draft_document(&app, &author, type_id, &format!("Number {index}")).await);
    }

    let summary = summary_of(&app, &author).await;
    let listed = recent_ids(&summary);

    assert_eq!(listed.len(), 5, "the widget carried {} rows", listed.len());

    let expected: Vec<String> = created
        .iter()
        .rev()
        .take(5)
        .map(|id| id.to_string())
        .collect();
    assert_eq!(
        listed, expected,
        "the five shown were not the five most recently touched"
    );
    assert!(
        !listed.contains(&created[0].to_string()),
        "the oldest touch was kept and a newer one dropped"
    );
    assert!(
        summary.get("recentDocumentsTotal").is_none(),
        "a count appeared beside the list; the widget deliberately has none"
    );
}

/// **A viewer who has touched nothing gets an empty array and not an error**
/// ([#433] AC5).
///
/// The server half of *the screen says so in words*: an empty list is a
/// successful answer, so the page can tell *you have touched nothing* from *the
/// card failed to load*. A summary that omitted the field, or refused, would
/// leave the page unable to distinguish them.
///
/// [#433]: https://github.com/sujanto-gaws/kelir/issues/433
#[tokio::test]
async fn a_viewer_who_has_touched_nothing_gets_an_empty_list() {
    let app = TestApp::spawn().await;

    let role = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "RD-RNONE",
        &["reporting:dashboard:read"],
    )
    .await;
    let token = user_with_roles(&app, "rd.rnone", &[role]).await;

    let response = app.get(SUMMARY, Some(&token)).await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.body);

    let recent = &response.body["data"]["recentDocuments"];
    assert!(
        recent.is_array(),
        "recentDocuments was not an array for a viewer with nothing: {recent}"
    );
    assert_eq!(recent.as_array().expect("an array").len(), 0);
}

/// **The widget is served to a caller holding only `reporting:dashboard:read`.**
///
/// [#433] AC7's permission boundary, and the field that makes it worth asserting
/// again rather than leaning on
/// [`the_summary_asks_for_no_permission_but_its_own`]: FR-RPT-003 puts document
/// **rows** — titles, numbers, references — behind a grant that is not
/// `document:read`.
///
/// That is deliberate and ADR-0039 took it: the caller is the *actor* on every
/// event that put a row in this list, so there is no title here they are
/// learning for the first time. Requiring `document:read` would refuse somebody
/// a list of their own work on the one screen built to show it —
/// `0041_activity_read_dropped.sql`'s mistake arriving from the other direction.
///
/// **The row is read back rather than the status alone**, because a summary that
/// served an empty array to this caller would pass a 200 assertion while having
/// quietly acquired a permission check.
///
/// [#433]: https://github.com/sujanto-gaws/kelir/issues/433
#[tokio::test]
async fn the_widget_asks_for_no_document_read() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let workflow = publish_workflow(&app, &token, "rd_noread", "RD-NOREAD").await;
    let type_id = document_type(&app, &token, "RD_NORD", workflow).await;

    // Raised with `document:create` and read back without `document:read`, so
    // the two grants are separated inside one test.
    let raiser_role = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "RD-NOREAD-RAISE",
        &["document:create", "document:read"],
    )
    .await;
    let dashboard_role = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "RD-NOREAD-DASH",
        &["reporting:dashboard:read"],
    )
    .await;

    let raising = user_with_roles(&app, "rd.noread", &[raiser_role]).await;
    let document = draft_document(&app, &raising, type_id, "Raised by its own author").await;

    // The same person, now holding the dashboard grant and nothing else.
    let user = user_id_of(&app, "rd.noread").await;
    sqlx::query("DELETE FROM user_roles WHERE user_id = $1")
        .bind(user)
        .execute(&app.pool)
        .await
        .expect("drop the raising role");
    sqlx::query("INSERT INTO user_roles (id, tenant_id, user_id, role_id) VALUES ($1, $2, $3, $4)")
        .bind(Uuid::now_v7())
        .bind(fixtures::SYSTEM_TENANT_ID)
        .bind(user)
        .bind(dashboard_role)
        .execute(&app.pool)
        .await
        .expect("grant only the dashboard");

    let dashboard_only = app.sign_in("rd.noread", common::ADMIN_PASSWORD).await;

    // The document surface is refused...
    let refused = app.get("/api/v1/documents", Some(&dashboard_only)).await;
    assert_eq!(
        refused.status,
        StatusCode::FORBIDDEN,
        "the fixture did not actually remove document:read: {}",
        refused.body
    );

    // ...and the widget still carries the row.
    assert_eq!(
        recent_ids(&summary_of(&app, &dashboard_only).await),
        vec![document.to_string()],
        "a caller holding only reporting:dashboard:read was refused their own work"
    );
}

/// **`lastTouchedAt` is the value the order was taken on.**
///
/// The field the row carries beyond `DocumentSummary`, and the one thing a
/// client must not recompute: it is `max(activity_events.created_at)` over the
/// caller's own touches, read in the statement that matched the row. A client
/// re-sorting on `updatedAt` would be a second opinion about what *recent*
/// means, and `updatedAt` moves when *anybody* changes the document.
///
/// Asserted as a property of the payload — descending, and each row's own
/// timestamp — rather than against fixture times, so it holds whatever the
/// clock did.
#[tokio::test]
async fn the_rows_carry_the_timestamp_they_were_ordered_by() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let workflow = publish_workflow(&app, &token, "rd_stamp", "RD-STAMP").await;
    let type_id = document_type(&app, &token, "RD_STMP", workflow).await;

    let author = holder(&app, "RD-STAMP", "rd.stamp").await;
    let author_id = user_id_of(&app, "rd.stamp").await;

    let older = draft_document(&app, &author, type_id, "Touched again later").await;
    draft_document(&app, &author, type_id, "Touched once").await;

    record_touch(
        &app,
        fixtures::SYSTEM_TENANT_ID,
        older,
        Some(author_id),
        "Document.Updated",
        Utc::now() + Duration::minutes(5),
    )
    .await;

    let summary = summary_of(&app, &author).await;
    let rows = summary["recentDocuments"].as_array().expect("an array");
    assert_eq!(rows.len(), 2);

    let stamps: Vec<chrono::DateTime<Utc>> = rows
        .iter()
        .map(|row| {
            row["lastTouchedAt"]
                .as_str()
                .unwrap_or_else(|| panic!("a row carries lastTouchedAt: {row}"))
                .parse()
                .expect("an RFC 3339 timestamp")
        })
        .collect();

    assert!(
        stamps[0] > stamps[1],
        "the rows were not ordered by lastTouchedAt descending: {stamps:?}"
    );

    // The document flattened onto the row is the list's own summary, so a field
    // the document list serves is on it too.
    assert_eq!(rows[0]["id"], older.to_string());
    assert!(
        rows[0]["documentRef"].is_string(),
        "the row is not a DocumentSummary: {}",
        rows[0]
    );
    assert_eq!(rows[0]["status"], "DRAFT");

    // And `lastTouchedAt` is the *latest* touch rather than the document's own
    // timestamps, which is what the re-float above proves it is not.
    let created_at: chrono::DateTime<Utc> = rows[0]["createdAt"]
        .as_str()
        .expect("createdAt")
        .parse()
        .expect("a timestamp");
    assert!(
        stamps[0] > created_at,
        "lastTouchedAt was the document's own createdAt"
    );
}

/// **A foreign tenant's event cannot date this tenant's document.**
///
/// # This test exists because a mutation survived
///
/// M3 — deleting `a.tenant_id = d.tenant_id` from the join — left all
/// twenty-seven tests green on the first run, and
/// [`a_second_tenants_document_is_not_recent_here`] was the test that should
/// have caught it and did not. **It could not have**, and the reason is worth
/// writing down: that test puts the foreign event on a foreign *document*, and
/// `d.tenant_id = $1` had already excluded the document before the join was
/// reached. It proves the `WHERE` and says nothing about the `ON`.
///
/// **The row that reaches the join is the mismatched one**: an event stamped
/// with another tenant, pointing at a document in *this* one.
/// `activity_events.document_id` has no composite foreign key tying the two
/// tenant columns together (`0033_activity.sql`), so nothing in the schema
/// forbids it.
///
/// **What it would cost is not a leaked row but a wrong date.** The document is
/// this tenant's and appears either way; without the predicate the foreign
/// event joins, wins the `max(a.created_at)`, and re-floats the document to the
/// top of somebody's widget on the strength of activity recorded in another
/// tenant. So the assertion is about *order and timestamp*, which is where the
/// damage would show, rather than about presence, which is where it would not.
#[tokio::test]
async fn a_foreign_tenants_event_cannot_date_this_tenants_document() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let workflow = publish_workflow(&app, &token, "rd_xten", "RD-XTEN").await;
    let type_id = document_type(&app, &token, "RD_XTN", workflow).await;

    let author = holder(&app, "RD-XTEN", "rd.xten").await;
    let author_id = user_id_of(&app, "rd.xten").await;

    let older = draft_document(&app, &author, type_id, "Raised first").await;
    let newer = draft_document(&app, &author, type_id, "Raised second").await;

    let other_tenant = fixtures::create_tenant(&app.pool, "RD-XTEN-OTHER", "Other tenant").await;

    // The mismatched row: this tenant's document, this caller as the actor, and
    // **another tenant's** `tenant_id`. Dated ahead of everything, so joining it
    // would be visible in the order rather than only in the timestamp.
    record_touch(
        &app,
        other_tenant,
        older,
        Some(author_id),
        "Document.Updated",
        Utc::now() + Duration::minutes(30),
    )
    .await;

    let summary = summary_of(&app, &author).await;

    assert_eq!(
        recent_ids(&summary),
        vec![newer.to_string(), older.to_string()],
        "an activity event stamped with another tenant re-floated this tenant's document"
    );

    // And the date the row carries is this tenant's own event, not the foreign
    // one — the half that would still be wrong if the order happened to survive.
    let rows = summary["recentDocuments"].as_array().expect("an array");
    let older_row = rows
        .iter()
        .find(|row| row["id"] == older.to_string())
        .expect("the older document is listed");
    let stamped: chrono::DateTime<Utc> = older_row["lastTouchedAt"]
        .as_str()
        .expect("lastTouchedAt")
        .parse()
        .expect("a timestamp");

    assert!(
        stamped < Utc::now() + Duration::minutes(30),
        "the row was dated by an event belonging to another tenant: {stamped}"
    );

    // The mismatched row really is in the table, so this is an assertion about
    // the join rather than about an insert that silently did nothing.
    let planted: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM activity_events WHERE document_id = $1 AND tenant_id = $2",
    )
    .bind(older)
    .bind(other_tenant)
    .fetch_one(&app.pool)
    .await
    .expect("read the planted event");
    assert_eq!(planted, 1, "the fixture did not plant the mismatched event");
}

/// **The recent-documents row is in the published contract, and it is flat.**
///
/// `RecentlyTouchedDocument` carries `DocumentSummary` under `#[serde(flatten)]`,
/// which serde and utoipa render two different ways: serde inlines the fields,
/// utoipa emits an `allOf`. **A generated client reads the second and a browser
/// receives the first**, so the two have to agree, and the way they stop
/// agreeing is a schema that was never registered — `components(schemas(...))`
/// in `router.rs` is a separate list from the `ToSchema` derive, and forgetting
/// it leaves a dangling `$ref` that nothing else in this suite would notice.
///
/// The wire shape is asserted by every other test in this file reading
/// `documentRef` off the row directly. **This asserts the contract half.**
#[tokio::test]
async fn the_recent_document_row_is_in_the_published_contract() {
    let app = TestApp::spawn().await;

    let document = app.get("/api/docs/openapi.json", None).await;
    assert_eq!(document.status, StatusCode::OK);

    let schemas = &document.body["components"]["schemas"];

    assert!(
        !schemas["RecentlyTouchedDocument"].is_null(),
        "RecentlyTouchedDocument is served on the dashboard and is not in the contract"
    );
    assert!(
        !schemas["DocumentSummary"].is_null(),
        "the flattened half is referenced and is not in the contract"
    );

    // And the summary names the field, so a client knows the widget exists at
    // all. `recentDocuments` rather than `recent_documents`: the envelope is
    // camelCase and the schema has to say so.
    let summary = &schemas["DashboardSummary"]["properties"];
    assert!(
        !summary["recentDocuments"].is_null(),
        "DashboardSummary does not carry recentDocuments: {summary}"
    );
    assert!(
        !summary["pendingTasks"].is_null(),
        "the contract lost a field this sprint already shipped"
    );
    // FR-RPT-005's rows (#446), on the same object — the field a generated
    // client reads to know the overdue card exists.
    assert!(
        !summary["overdueTasks"].is_null(),
        "DashboardSummary does not carry overdueTasks: {summary}"
    );
    // FR-RPT-004's counts (#447), and the element type they reference.
    assert!(
        !summary["documentsByStatus"].is_null(),
        "DashboardSummary does not carry documentsByStatus: {summary}"
    );
    assert!(
        !schemas["DocumentStatusCount"].is_null(),
        "documentsByStatus references DocumentStatusCount and it is not in the contract"
    );

    // There is still exactly one dashboard path (#433 AC1, ADR-0039) — the
    // decision this row was most likely to reverse, asserted in the contract
    // rather than only in the frontend's request log.
    let paths = document.body["paths"]
        .as_object()
        .expect("the contract has paths");
    let dashboard: Vec<&String> = paths
        .keys()
        .filter(|path| path.contains("/dashboard"))
        .collect();

    assert_eq!(
        dashboard,
        vec!["/api/v1/dashboard/summary"],
        "a second dashboard endpoint appeared beside the summary"
    );
}

// ---------------------------------------------------------------------------
// FR-RPT-005 — the overdue-tasks widget ([#446])
// ---------------------------------------------------------------------------

/// The ids of the tasks the overdue widget listed, in the order it listed them.
fn overdue_ids(summary: &Value) -> Vec<String> {
    summary["overdueTasks"]
        .as_array()
        .unwrap_or_else(|| panic!("the summary carries an overdueTasks array: {summary}"))
        .iter()
        .map(|task| {
            task["id"]
                .as_str()
                .unwrap_or_else(|| panic!("an overdue task has an id: {task}"))
                .to_owned()
        })
        .collect()
}

fn sorted(mut ids: Vec<String>) -> Vec<String> {
    ids.sort();
    ids
}

/// What the **inbox** calls overdue for this caller: the ids on its
/// `?scope=overdue` page, in the inbox's order, and the `meta.total` beside
/// them.
///
/// The widget is asserted against this rather than against a list the fixture
/// wrote down, for the reason [`inbox_ids`] gives ([#432] AC2). **As a set**,
/// because the inbox pages newest first and the widget is longest late first —
/// the same rows in a different sequence is the design, not a divergence.
async fn inbox_overdue(app: &TestApp, token: &str) -> (Vec<String>, Value) {
    let response = app
        .get(&format!("{TASKS}?scope=overdue"), Some(token))
        .await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.body);

    let ids = response.body["data"]
        .as_array()
        .expect("the inbox is a page")
        .iter()
        .map(|task| task["id"].as_str().expect("an id").to_owned())
        .collect();

    (ids, response.body["meta"]["total"].clone())
}

/// The open task a document raised.
async fn open_task_of(app: &TestApp, document: Uuid) -> String {
    let id: Uuid = sqlx::query_scalar(
        "SELECT id FROM workflow_tasks WHERE document_id = $1 \
         AND status IN ('CREATED','ASSIGNED','IN_PROGRESS')",
    )
    .bind(document)
    .fetch_one(&app.pool)
    .await
    .expect("read the open task");

    id.to_string()
}

/// Dates the open task on `document` as due `hours` hours ago, and returns it.
///
/// Set by an `UPDATE`, as [`overdue_counts_only_what_is_late`] does, because
/// what is under test is the widget rather than the engine's own due-date
/// derivation — which `workflow_due_dates.rs` owns. Against the database's
/// `now()`, so the fixture and the predicate read one clock.
async fn late_by(app: &TestApp, document: Uuid, hours: i32) -> String {
    let task = open_task_of(app, document).await;

    let updated = sqlx::query(
        "UPDATE workflow_tasks SET due_at = now() - make_interval(hours => $2) \
         WHERE id = $1::uuid",
    )
    .bind(&task)
    .bind(hours)
    .execute(&app.pool)
    .await
    .expect("set a due date in the past");
    assert_eq!(updated.rows_affected(), 1, "the fixture dated no task");

    task
}

/// Gives `task` exactly the due date `other` has, so the two tie on it.
async fn same_due_as(app: &TestApp, task: &str, other: &str) {
    let updated = sqlx::query(
        "UPDATE workflow_tasks SET due_at = (SELECT due_at FROM workflow_tasks WHERE id = $2::uuid) \
         WHERE id = $1::uuid",
    )
    .bind(task)
    .bind(other)
    .execute(&app.pool)
    .await
    .expect("tie two due dates");
    assert_eq!(updated.rows_affected(), 1, "the fixture tied no task");
}

/// **The overdue widget lists what the inbox calls overdue, and counts what the
/// inbox totals** ([#446] AC3).
///
/// # A divergence test, for the reason the pending-task one is
///
/// The expected value is `GET /api/v1/tasks?scope=overdue`, read in the same
/// test: the ids as a set, and `meta.total` against `tasksOverdue`. A widget
/// that had grown its own idea of *late* — or of *whose* — disagrees with that
/// page whichever way round it falls, and a test that merely found some late
/// rows on the card would pass for it.
///
/// **The fixture has the three tasks a wrong rule would admit**: a late task of
/// a second holder, which a widget without the visibility rule lists; and an
/// undated task of this holder's own, which a widget reading *open* rather than
/// *overdue* lists.
///
/// [#446]: https://github.com/sujanto-gaws/kelir/issues/446
#[tokio::test]
async fn the_overdue_widget_lists_what_the_inbox_calls_overdue() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let theirs = holder(&app, "RD-O-SAME", "rd.o.same").await;
    let other = holder(&app, "RD-O-OTHER", "rd.o.other").await;

    let workflow = publish_workflow(&app, &token, "rd_o_same", "RD-O-SAME").await;
    let other_workflow = publish_workflow(&app, &token, "rd_o_other", "RD-O-OTHER").await;
    let type_id = document_type(&app, &token, "RD_O_SAME", workflow).await;
    let other_type = document_type(&app, &token, "RD_O_OTH", other_workflow).await;

    let very_late = submitted_document(&app, &token, type_id, "Two days late").await;
    let bit_late = submitted_document(&app, &token, type_id, "Three hours late").await;
    submitted_document(&app, &token, type_id, "Waiting, with no date").await;
    let elsewhere = submitted_document(&app, &token, other_type, "Late, and not theirs").await;

    let very_late = late_by(&app, very_late, 48).await;
    let bit_late = late_by(&app, bit_late, 3).await;
    let not_theirs = late_by(&app, elsewhere, 96).await;

    let (inbox, total) = inbox_overdue(&app, &theirs).await;
    let summary = summary_of(&app, &theirs).await;
    let listed = overdue_ids(&summary);

    assert_eq!(
        sorted(inbox.clone()),
        sorted(vec![very_late, bit_late]),
        "the fixture did not make the tasks late it meant to"
    );
    assert_eq!(
        sorted(listed.clone()),
        sorted(inbox),
        "the overdue widget and the inbox's overdue view disagree about which \
         tasks are late — two answers to whose late task this is: {summary}"
    );
    assert_eq!(
        summary["tasksOverdue"], total,
        "the card's count and the inbox's total disagree: {summary}"
    );
    assert!(
        !listed.contains(&not_theirs),
        "another holder's late task was on this caller's overdue widget: {summary}"
    );
    // The undated task is still this caller's and still waiting, so its absence
    // above is the date doing it rather than the task never being visible.
    assert_eq!(summary["tasksWaiting"], 3, "{summary}");

    // And the other holder's, so the assertions above are not passing because
    // every late row in this test belongs to everybody.
    let (other_inbox, other_total) = inbox_overdue(&app, &other).await;
    let other_summary = summary_of(&app, &other).await;

    assert_eq!(other_inbox, vec![not_theirs]);
    assert_eq!(
        overdue_ids(&other_summary),
        other_inbox,
        "the second holder's overdue widget does not match their own inbox: {other_summary}"
    );
    assert_eq!(other_summary["tasksOverdue"], other_total);
}

/// **Late means dated and still open: an undated task is not overdue, and
/// neither is a late one somebody finished** ([#446] AC2, both halves).
///
/// The finished task is **on the widget first** and decided through
/// `POST /api/v1/workflow/tasks/{id}/decision`, so the assertion is that it
/// *left* rather than that it was never there, and the row reaches the state
/// the product produces rather than one an `UPDATE` of `status` invented. Its
/// date has still passed afterwards — read back from the table — so what takes
/// it off the card is the status and nothing else.
///
/// [#446]: https://github.com/sujanto-gaws/kelir/issues/446
#[tokio::test]
async fn an_undated_task_and_a_finished_late_one_are_not_overdue() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let approver = holder(&app, "RD-O-AC2", "rd.o.ac2").await;
    let workflow = publish_workflow(&app, &token, "rd_o_ac2", "RD-O-AC2").await;
    let type_id = document_type(&app, &token, "RD_O_AC2", workflow).await;

    let late = submitted_document(&app, &token, type_id, "Late, still open").await;
    let undated = submitted_document(&app, &token, type_id, "Open, no date").await;
    let finished = submitted_document(&app, &token, type_id, "Late, then decided").await;

    let late = late_by(&app, late, 24).await;
    let undated = open_task_of(&app, undated).await;
    let finished = late_by(&app, finished, 48).await;

    let before = summary_of(&app, &approver).await;
    assert_eq!(
        overdue_ids(&before),
        vec![finished.clone(), late.clone()],
        "the fixture's two late tasks were not both on the widget before the decision: {before}"
    );

    let decided = app
        .post(
            &format!("/api/v1/workflow/tasks/{finished}/decision"),
            Some(&approver),
            json!({ "action": "APPROVE" }),
        )
        .await;
    assert_eq!(decided.status, StatusCode::OK, "{}", decided.body);

    let (status, date_passed): (String, bool) =
        sqlx::query_as("SELECT status, due_at < now() FROM workflow_tasks WHERE id = $1::uuid")
            .bind(&finished)
            .fetch_one(&app.pool)
            .await
            .expect("read the decided task");
    assert_eq!(status, "COMPLETED");
    assert!(
        date_passed,
        "the decided task's date no longer reads as passed"
    );

    let after = summary_of(&app, &approver).await;
    let listed = overdue_ids(&after);

    assert!(
        !listed.contains(&undated),
        "a task with no due date was listed as overdue: {after}"
    );
    assert!(
        !listed.contains(&finished),
        "a task finished after its date passed was still listed as overdue: {after}"
    );
    assert_eq!(listed, vec![late], "{after}");
    assert_eq!(after["tasksOverdue"], 1, "{after}");
    // The undated task is still waiting, so it is visible and merely not late.
    assert_eq!(after["tasksWaiting"], 2, "{after}");
}

/// **The task late longest comes first** — and that is not the inbox's order.
///
/// Five tasks raised in one order and dated in another, with **four dating
/// moves and a tie** (coding standard §2.9's three-move rule). The due order is
/// neither the order they were raised in nor its reverse, so a widget that kept
/// the inbox's newest-first order, sorted `due_at` the wrong way, or ignored
/// the date altogether produces a different list than the one asserted.
///
/// **The tie is broken the inbox's way** — `created_at DESC, id DESC` — so the
/// order is total and a card reloaded twice lists the same rows in the same
/// sequence.
///
/// [#446]: https://github.com/sujanto-gaws/kelir/issues/446
#[tokio::test]
async fn the_task_late_longest_comes_first() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let approver = holder(&app, "RD-O-ORDER", "rd.o.order").await;
    let workflow = publish_workflow(&app, &token, "rd_o_order", "RD-O-ORDER").await;
    let type_id = document_type(&app, &token, "RD_O_ORD", workflow).await;

    let mut raised = Vec::new();
    for n in 1..=5 {
        raised.push(submitted_document(&app, &token, type_id, &format!("Raised {n}")).await);
    }

    let first = late_by(&app, raised[0], 2).await;
    let second = late_by(&app, raised[1], 72).await;
    let third = late_by(&app, raised[2], 1).await;
    let fourth = late_by(&app, raised[3], 24).await;
    // Exactly as late as the second, so only the tie-break can order the two.
    let fifth = open_task_of(&app, raised[4]).await;
    same_due_as(&app, &fifth, &second).await;

    let summary = summary_of(&app, &approver).await;
    let listed = overdue_ids(&summary);

    assert_eq!(
        listed,
        vec![
            fifth.clone(),
            second.clone(),
            fourth.clone(),
            first.clone(),
            third.clone()
        ],
        "the overdue widget is not longest late first, ties newest first: {summary}"
    );

    // The same rows as the inbox's overdue view, and **not** in its order —
    // which is newest first, and here would be fifth, fourth, third, second,
    // first.
    let (inbox, _) = inbox_overdue(&app, &approver).await;
    assert_eq!(sorted(listed.clone()), sorted(inbox.clone()));
    assert_ne!(
        listed, inbox,
        "the overdue widget kept the inbox's newest-first order"
    );

    // And as a property of the payload: `dueAt` never goes backwards.
    let due: Vec<chrono::DateTime<Utc>> = summary["overdueTasks"]
        .as_array()
        .expect("an array")
        .iter()
        .map(|task| {
            task["dueAt"]
                .as_str()
                .unwrap_or_else(|| panic!("an overdue task has a dueAt: {task}"))
                .parse()
                .expect("an RFC 3339 timestamp")
        })
        .collect();
    assert!(
        due.windows(2).all(|pair| pair[0] <= pair[1]),
        "dueAt is not ascending down the card: {due:?}"
    );
    assert!(
        summary["overdueTasks"]
            .as_array()
            .expect("an array")
            .iter()
            .all(|task| task["isOverdue"] == true),
        "a row on the overdue card does not say it is overdue: {summary}"
    );
}

/// **Six late, five rows, and the count says six** ([#446]).
///
/// `OVERDUE_TASKS_SHOWN`. The row left off is **the least late**, and it was
/// raised third — neither the oldest nor the newest — so a card capped in the
/// inbox's order, or in raising order, drops a different task. `tasksOverdue`
/// is six because it counts the late set rather than the card, which is what
/// lets a client write *1 more late* as `tasksOverdue - overdueTasks.length`.
///
/// [#446]: https://github.com/sujanto-gaws/kelir/issues/446
#[tokio::test]
async fn the_card_shows_the_five_longest_late_and_counts_all_six() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let approver = holder(&app, "RD-O-CAP", "rd.o.cap").await;
    let workflow = publish_workflow(&app, &token, "rd_o_cap", "RD-O-CAP").await;
    let type_id = document_type(&app, &token, "RD_O_CAP", workflow).await;

    let lateness = [5, 60, 1, 30, 10, 3];
    let mut tasks = Vec::new();
    for (n, hours) in lateness.iter().enumerate() {
        let document = submitted_document(&app, &token, type_id, &format!("Late {n}")).await;
        tasks.push(late_by(&app, document, *hours).await);
    }

    let summary = summary_of(&app, &approver).await;
    let listed = overdue_ids(&summary);

    assert_eq!(
        summary["tasksOverdue"], 6,
        "the count describes the late set, not the card: {summary}"
    );
    assert_eq!(
        listed,
        vec![
            tasks[1].clone(),
            tasks[3].clone(),
            tasks[4].clone(),
            tasks[0].clone(),
            tasks[5].clone()
        ],
        "the card does not carry the five longest late, longest first: {summary}"
    );
    assert!(
        !listed.contains(&tasks[2]),
        "the least late task was kept and a later one dropped: {summary}"
    );

    let (inbox, total) = inbox_overdue(&app, &approver).await;
    assert_eq!(inbox.len(), 6, "the fixture did not make six tasks late");
    assert_eq!(summary["tasksOverdue"], total);
}

/// **Nothing late is an empty list beside a zero** ([#446]).
///
/// The caller has work waiting — an undated task — and somebody else has a late
/// one, so this asserts *nothing late for this caller* rather than *nothing
/// waiting* or *nothing late in the tenant*.
///
/// [#446]: https://github.com/sujanto-gaws/kelir/issues/446
#[tokio::test]
async fn a_viewer_with_nothing_late_gets_an_empty_list_and_a_zero() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let idle = holder(&app, "RD-O-IDLE", "rd.o.idle").await;
    let busy = holder(&app, "RD-O-BUSY", "rd.o.busy").await;

    let idle_workflow = publish_workflow(&app, &token, "rd_o_idle", "RD-O-IDLE").await;
    let busy_workflow = publish_workflow(&app, &token, "rd_o_busy", "RD-O-BUSY").await;
    let idle_type = document_type(&app, &token, "RD_O_IDLE", idle_workflow).await;
    let busy_type = document_type(&app, &token, "RD_O_BUSY", busy_workflow).await;

    submitted_document(&app, &token, idle_type, "Waiting, not late").await;
    let late = submitted_document(&app, &token, busy_type, "Somebody else's, late").await;
    late_by(&app, late, 24).await;

    let summary = summary_of(&app, &idle).await;

    assert!(
        summary["overdueTasks"].is_array(),
        "overdueTasks is an array even when it is empty: {summary}"
    );
    assert_eq!(overdue_ids(&summary), Vec::<String>::new());
    assert_eq!(summary["tasksOverdue"], 0, "{summary}");
    assert_eq!(
        summary["tasksWaiting"], 1,
        "the caller's own undated task was not raised, so the empty above means less: {summary}"
    );

    assert_eq!(
        overdue_ids(&summary_of(&app, &busy).await).len(),
        1,
        "the fixture's late task was not on its own holder's widget, so the empty above means nothing"
    );
}

/// **A department-scoped late task reaches only that department's overdue
/// widget** ([#446] AC3).
///
/// The same fixture as
/// [`a_department_scoped_task_reaches_only_that_departments_widget`], made late.
/// **Inherited rather than restated**: the widget reads the inbox's statement,
/// so the `candidate_department_id` clause [#225] added is the one thing
/// deciding this — and this test is what fails if a later author gives the
/// overdue card a query of its own that leaves the clause out.
///
/// [#225]: https://github.com/sujanto-gaws/kelir/issues/225
/// [#446]: https://github.com/sujanto-gaws/kelir/issues/446
#[tokio::test]
async fn a_department_scoped_late_task_reaches_only_that_departments_overdue_widget() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let finance = department(&app, "RD-O-DEPT-FIN", "Finance").await;
    let procurement = department(&app, "RD-O-DEPT-PROC", "Procurement").await;

    let role = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "RD-O-DEPT",
        HOLDER_PERMISSIONS,
    )
    .await;

    let insider = user_with_roles(&app, "rd.o.fin", &[role]).await;
    scope_grant_to(&app, "rd.o.fin", finance).await;
    let outsider = user_with_roles(&app, "rd.o.proc", &[role]).await;
    scope_grant_to(&app, "rd.o.proc", procurement).await;

    let workflow = publish_workflow_definition(
        &app,
        &token,
        "rd_o_dept",
        department_workflow("rd_o_dept", "RD-O-DEPT", "RD-O-DEPT-FIN"),
    )
    .await;
    let type_id = document_type(&app, &token, "RD_O_DEPT", workflow).await;

    let document = submitted_document(&app, &token, type_id, "Finance's, late").await;
    let task = late_by(&app, document, 24).await;

    let theirs = summary_of(&app, &insider).await;
    let not_theirs = summary_of(&app, &outsider).await;

    assert_eq!(
        overdue_ids(&theirs),
        vec![task],
        "the department's own approver was not shown their late task: {theirs}"
    );
    assert_eq!(
        overdue_ids(&theirs),
        inbox_overdue(&app, &insider).await.0,
        "the department's approver sees different late work on the two surfaces: {theirs}"
    );
    assert_eq!(
        overdue_ids(&not_theirs),
        Vec::<String>::new(),
        "Procurement's approver was shown Finance's late task: {not_theirs}"
    );
    assert_eq!(
        not_theirs["tasksOverdue"], 0,
        "the overdue count crossed the department boundary: {not_theirs}"
    );
}

/// **A delegated late task is on the delegate's overdue widget, and off the
/// delegator's** ([#446] AC3).
///
/// The same fixture as
/// [`a_delegated_task_is_on_the_delegates_widget_and_not_the_delegators`], made
/// late — and the case where forgetting delegation costs most, because the
/// person standing in is the one who has to know the work is *late*.
///
/// [#446]: https://github.com/sujanto-gaws/kelir/issues/446
#[tokio::test]
async fn a_delegated_late_task_is_on_the_delegates_overdue_widget_and_not_the_delegators() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let (delegator_id, delegator) = holder_party(
        &app,
        "RD-O-DELEGATOR",
        "rd.o.ani",
        &["identity:delegation:create"],
    )
    .await;
    let (delegate_id, delegate) = holder_party(&app, "RD-O-DELEGATE", "rd.o.budi", &[]).await;

    let opened = app
        .post(
            "/api/v1/identity/delegations",
            Some(&delegator),
            json!({
                "delegateUserId": delegate_id,
                "startsAt": (Utc::now() - Duration::hours(1)).to_rfc3339(),
                "endsAt": (Utc::now() + Duration::hours(1)).to_rfc3339(),
            }),
        )
        .await;
    assert_eq!(opened.status, StatusCode::CREATED, "{}", opened.body);

    let workflow = publish_workflow_definition(
        &app,
        &token,
        "rd_o_deleg",
        user_workflow("rd_o_deleg", delegator_id),
    )
    .await;
    let type_id = document_type(&app, &token, "RD_O_DELEG", workflow).await;

    let document = submitted_document(&app, &token, type_id, "Late, on Ani's behalf").await;
    let task = late_by(&app, document, 24).await;

    let theirs = summary_of(&app, &delegate).await;
    let handed_over = summary_of(&app, &delegator).await;

    assert_eq!(
        overdue_ids(&theirs),
        vec![task],
        "the delegate's overdue widget is empty beside a late task they hold: {theirs}"
    );
    assert_eq!(
        overdue_ids(&theirs),
        inbox_overdue(&app, &delegate).await.0,
        "the delegate's overdue widget and overdue inbox disagree: {theirs}"
    );
    assert_eq!(
        theirs["overdueTasks"][0]["delegatedFromUserId"],
        json!(delegator_id.to_string()),
        "the late row does not say whose approval it is: {theirs}"
    );
    assert_eq!(
        overdue_ids(&handed_over),
        Vec::<String>::new(),
        "the late task is still on the overdue widget of the person who handed it over: {handed_over}"
    );
    assert_eq!(handed_over["tasksOverdue"], 0, "{handed_over}");
}

// ---------------------------------------------------------------------------
// FR-RPT-004 — the documents-by-status widget ([#447])
// ---------------------------------------------------------------------------

/// The ten statuses in `DocumentStatus`'s declaration order, **written out here
/// rather than read from the crate** — so reordering the enum reddens these
/// tests instead of quietly moving the expectation along with it.
const STATUS_AXIS: [&str; 10] = [
    "DRAFT",
    "SUBMITTED",
    "IN_REVIEW",
    "PENDING_APPROVAL",
    "APPROVED",
    "REJECTED",
    "RETURNED",
    "COMPLETED",
    "ARCHIVED",
    "CANCELLED",
];

/// `documentsByStatus` as `(status, count)` pairs, in the order it was served.
fn by_status(summary: &Value) -> Vec<(String, i64)> {
    summary["documentsByStatus"]
        .as_array()
        .unwrap_or_else(|| panic!("the summary carries a documentsByStatus array: {summary}"))
        .iter()
        .map(|entry| {
            (
                entry["status"]
                    .as_str()
                    .unwrap_or_else(|| panic!("an entry has a status: {entry}"))
                    .to_owned(),
                entry["count"]
                    .as_i64()
                    .unwrap_or_else(|| panic!("an entry has a count: {entry}")),
            )
        })
        .collect()
}

/// The whole axis, with the counts given and zero everywhere else.
fn axis_with(counts: &[(&str, i64)]) -> Vec<(String, i64)> {
    STATUS_AXIS
        .iter()
        .map(|&status| {
            let count = counts
                .iter()
                .find(|(named, _)| *named == status)
                .map_or(0, |(_, count)| *count);
            (status.to_owned(), count)
        })
        .collect()
}

/// Writes a document directly: any status, any author (or none), any tenant,
/// live or soft-deleted.
///
/// **Most of the ten statuses are reachable only by running a workflow to
/// them**, and what is under test is the count rather than the engine — the
/// reason [`late_by`] dates a task with an `UPDATE`. The other three shapes, a
/// null author, another tenant and a soft delete, are the ones a dropped
/// predicate would expose and no API call produces at all.
async fn plant_document(
    app: &TestApp,
    tenant_id: Uuid,
    created_by: Option<Uuid>,
    type_id: Uuid,
    status: &str,
    deleted: bool,
) -> Uuid {
    let id = Uuid::now_v7();

    sqlx::query(
        "INSERT INTO documents (id, tenant_id, created_by, document_ref, document_type_id, \
         title, status, deleted_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, CASE WHEN $8 THEN now() END)",
    )
    .bind(id)
    .bind(tenant_id)
    .bind(created_by)
    .bind(format!("RD-S-{id}"))
    .bind(type_id)
    .bind(format!("Planted, {status}"))
    .bind(status)
    .bind(deleted)
    .execute(&app.pool)
    .await
    .expect("plant a document");

    id
}

/// **The caller's documents, counted in every status, on a fixed axis**
/// ([#447] AC1).
///
/// # What the array promises a client
///
/// **All ten statuses, zeros included, in declaration order.** A chart draws
/// its axis from this array without knowing the enum, so an entry missing
/// because nothing is in it — or the ten arriving in whatever order a
/// `GROUP BY` produced them — is a chart that changes shape between two
/// callers.
///
/// **The fixture mixes both ways a document gets its status.** Two drafts and
/// one submission go through the API, so `DRAFT` and `PENDING_APPROVAL` are
/// the states the product actually produces; the rest are planted. The counts
/// are uneven and scattered — 2, 1, 3, 1, 1 across positions 0, 3, 4, 6 and 9
/// — so a fill that ran in arrival order, in reverse, or by count reads
/// differently from the one asserted.
///
/// [#447]: https://github.com/sujanto-gaws/kelir/issues/447
#[tokio::test]
async fn the_status_summary_counts_the_callers_documents_on_a_fixed_axis() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let workflow = publish_workflow(&app, &token, "rd_s_axis", "RD-S-AXIS").await;
    let type_id = document_type(&app, &token, "RD_S_AXIS", workflow).await;

    let (author_id, author) = holder_party(&app, "RD-S-AXIS", "rd.s.axis", &[]).await;

    draft_document(&app, &author, type_id, "Draft one").await;
    draft_document(&app, &author, type_id, "Draft two").await;
    let sent = submitted_document(&app, &author, type_id, "Sent for approval").await;

    let tenant = fixtures::SYSTEM_TENANT_ID;
    for status in ["APPROVED", "CANCELLED", "APPROVED", "RETURNED", "APPROVED"] {
        plant_document(&app, tenant, Some(author_id), type_id, status, false).await;
    }

    // The submission's status is the engine's, so it is read back rather than
    // assumed: `workflow_for`'s initial state maps to PENDING_APPROVAL.
    let sent_status: String = sqlx::query_scalar("SELECT status FROM documents WHERE id = $1")
        .bind(sent)
        .fetch_one(&app.pool)
        .await
        .expect("read the submitted document's status");
    assert_eq!(sent_status, "PENDING_APPROVAL", "the fixture's submission");

    let summary = summary_of(&app, &author).await;
    let served = by_status(&summary);

    assert_eq!(
        served
            .iter()
            .map(|(status, _)| status.as_str())
            .collect::<Vec<_>>(),
        STATUS_AXIS,
        "documentsByStatus is not every status in declaration order: {summary}"
    );
    assert_eq!(
        served,
        axis_with(&[
            ("DRAFT", 2),
            ("PENDING_APPROVAL", 1),
            ("APPROVED", 3),
            ("RETURNED", 1),
            ("CANCELLED", 1),
        ]),
        "the caller's documents were miscounted: {summary}"
    );
}

/// **Another author's documents are not in the caller's status summary**
/// ([#447] AC2).
///
/// Two authors in one tenant, asserted both ways round, because one subject
/// cannot tell *the caller's documents* from *the tenant's* (coding standard
/// §2.9): with `created_by` gone, both read `DRAFT 3, APPROVED 1`.
///
/// [#447]: https://github.com/sujanto-gaws/kelir/issues/447
#[tokio::test]
async fn another_authors_documents_are_not_in_the_callers_status_summary() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let workflow = publish_workflow(&app, &token, "rd_s_who", "RD-S-WHO").await;
    let type_id = document_type(&app, &token, "RD_S_WHO", workflow).await;

    let ani = holder(&app, "RD-S-ANI", "rd.s.ani").await;
    let (budi_id, budi) = holder_party(&app, "RD-S-BUDI", "rd.s.budi", &[]).await;

    draft_document(&app, &ani, type_id, "Ani's draft").await;
    draft_document(&app, &budi, type_id, "Budi's draft").await;
    draft_document(&app, &budi, type_id, "Budi's second draft").await;
    plant_document(
        &app,
        fixtures::SYSTEM_TENANT_ID,
        Some(budi_id),
        type_id,
        "APPROVED",
        false,
    )
    .await;

    assert_eq!(
        by_status(&summary_of(&app, &ani).await),
        axis_with(&[("DRAFT", 1)]),
        "Ani's status summary counted somebody else's documents"
    );
    assert_eq!(
        by_status(&summary_of(&app, &budi).await),
        axis_with(&[("DRAFT", 2), ("APPROVED", 1)]),
        "Budi's status summary lost his own documents or gained Ani's"
    );
}

/// **A second tenant's documents are not in the status summary** ([#447] AC2).
///
/// Planted under another tenant **with this caller as the author**, because
/// that is the only row a dropped `tenant_id` would expose: the document
/// endpoints stamp the caller's own tenant, so a fixture that went through them
/// could never tell a tenant-scoped count from an unscoped one.
///
/// [#447]: https://github.com/sujanto-gaws/kelir/issues/447
#[tokio::test]
async fn a_second_tenants_documents_are_not_in_the_status_summary() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let workflow = publish_workflow(&app, &token, "rd_s_ten", "RD-S-TEN").await;
    let type_id = document_type(&app, &token, "RD_S_TEN", workflow).await;

    let (author_id, author) = holder_party(&app, "RD-S-TEN", "rd.s.ten", &[]).await;
    draft_document(&app, &author, type_id, "In their own tenant").await;

    let other_tenant = fixtures::create_tenant(&app.pool, "RD-S-OTHER", "Other tenant").await;
    for status in ["DRAFT", "APPROVED"] {
        plant_document(&app, other_tenant, Some(author_id), type_id, status, false).await;
    }

    assert_eq!(
        by_status(&summary_of(&app, &author).await),
        axis_with(&[("DRAFT", 1)]),
        "documents filed under another tenant were counted on this caller's dashboard"
    );
}

/// **A soft-deleted document is not in the status summary** ([#447] AC2).
///
/// Two `COMPLETED` documents, one of them deleted, so the assertion is *one*
/// rather than *none* — a count that collapsed to zero would not pass it. The
/// deleted row is read back, so this is about the statement and not about an
/// insert that did nothing.
///
/// [#447]: https://github.com/sujanto-gaws/kelir/issues/447
#[tokio::test]
async fn a_soft_deleted_document_is_not_in_the_status_summary() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let workflow = publish_workflow(&app, &token, "rd_s_gone", "RD-S-GONE").await;
    let type_id = document_type(&app, &token, "RD_S_GONE", workflow).await;

    let (author_id, author) = holder_party(&app, "RD-S-GONE", "rd.s.gone", &[]).await;
    let tenant = fixtures::SYSTEM_TENANT_ID;

    plant_document(&app, tenant, Some(author_id), type_id, "COMPLETED", false).await;
    let deleted = plant_document(&app, tenant, Some(author_id), type_id, "COMPLETED", true).await;

    let is_deleted: bool =
        sqlx::query_scalar("SELECT deleted_at IS NOT NULL FROM documents WHERE id = $1")
            .bind(deleted)
            .fetch_one(&app.pool)
            .await
            .expect("read the deleted document");
    assert!(is_deleted, "the fixture did not soft-delete the document");

    assert_eq!(
        by_status(&summary_of(&app, &author).await),
        axis_with(&[("COMPLETED", 1)]),
        "a soft-deleted document was counted"
    );
}

/// **A document nobody raised is in nobody's status summary** ([#447] AC2).
///
/// `documents.created_by` is nullable, and a document the system raised has no
/// author. The predicate is an equality, so a null matches no caller; the
/// looser spelling — *the caller's, or nobody's* — would put every
/// system-raised document on every dashboard in the tenant, and this is the
/// only test here whose fixture has such a row.
///
/// [#447]: https://github.com/sujanto-gaws/kelir/issues/447
#[tokio::test]
async fn a_document_nobody_raised_is_in_nobodys_status_summary() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let workflow = publish_workflow(&app, &token, "rd_s_sys", "RD-S-SYS").await;
    let type_id = document_type(&app, &token, "RD_S_SYS", workflow).await;

    let author = holder(&app, "RD-S-SYS", "rd.s.sys").await;
    draft_document(&app, &author, type_id, "Raised by a person").await;

    for status in ["DRAFT", "APPROVED"] {
        plant_document(
            &app,
            fixtures::SYSTEM_TENANT_ID,
            None,
            type_id,
            status,
            false,
        )
        .await;
    }

    assert_eq!(
        by_status(&summary_of(&app, &author).await),
        axis_with(&[("DRAFT", 1)]),
        "a document with no author was counted on a person's dashboard"
    );
}

/// **`draftDocuments` is the `DRAFT` entry, and the two cannot disagree**
/// ([#447]).
///
/// One read serves both — the card's number and the chart's first bar — so
/// this is the assertion that they are one number. The caller's other statuses
/// hold counts that are not two, so reading `draftDocuments` off any other
/// entry fails here rather than agreeing by accident.
///
/// [#447]: https://github.com/sujanto-gaws/kelir/issues/447
#[tokio::test]
async fn draft_documents_is_the_draft_entry_of_the_status_summary() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let workflow = publish_workflow(&app, &token, "rd_s_one", "RD-S-ONE").await;
    let type_id = document_type(&app, &token, "RD_S_ONE", workflow).await;

    let (author_id, author) = holder_party(&app, "RD-S-ONE", "rd.s.one", &[]).await;
    draft_document(&app, &author, type_id, "Draft one").await;
    draft_document(&app, &author, type_id, "Draft two").await;

    let tenant = fixtures::SYSTEM_TENANT_ID;
    for status in ["RETURNED", "APPROVED", "APPROVED", "APPROVED"] {
        plant_document(&app, tenant, Some(author_id), type_id, status, false).await;
    }

    let summary = summary_of(&app, &author).await;
    let draft_entry = by_status(&summary)
        .into_iter()
        .find(|(status, _)| status == "DRAFT")
        .map(|(_, count)| count);

    assert_eq!(draft_entry, Some(2), "the DRAFT entry: {summary}");
    assert_eq!(
        summary["draftDocuments"].as_i64(),
        draft_entry,
        "draftDocuments and the DRAFT entry disagree — two answers to how many drafts: {summary}"
    );
}

// ---------------------------------------------------------------------------
// FR-RPT-006 — the approval time card ([#461])
// ---------------------------------------------------------------------------

const DAY: i64 = 86_400;

/// A workflow whose states cover every way an instance can end, so a planted
/// round can stand in any of them: decided either way, sent back, or withdrawn.
///
/// `RETURNED` is **not** final and has a `RESUBMIT` edge out of it, which is the
/// shape the product runs a return on — the instance stays live and the
/// resubmission moves it — and the one
/// [`a_returned_and_resubmitted_document_is_timed_from_its_first_submission`]
/// drives through the engine.
fn approval_time_workflow(key: &str, role_code: &str) -> Value {
    json!({
        "workflowKey": key,
        "version": "1.0.0",
        "name": "Approval that can send back or be withdrawn",
        "initialState": "MANAGER_APPROVAL",
        "states": [
            { "code": "MANAGER_APPROVAL", "name": "Manager approval",
              "mapsToDocumentStatus": "PENDING_APPROVAL",
              "task": { "taskDefinitionKey": "manager_approval",
                        "taskName": "Decide the request",
                        "assignment": { "assigneeType": "ROLE", "roleCode": role_code } } },
            { "code": "RETURNED", "name": "Sent back", "mapsToDocumentStatus": "RETURNED" },
            { "code": "COMPLETED", "name": "Completed", "mapsToDocumentStatus": "COMPLETED",
              "isFinal": true },
            { "code": "REJECTED", "name": "Rejected", "mapsToDocumentStatus": "REJECTED",
              "isFinal": true },
            { "code": "WITHDRAWN", "name": "Withdrawn", "mapsToDocumentStatus": "CANCELLED",
              "isFinal": true }
        ],
        "transitions": [
            { "from": "MANAGER_APPROVAL", "to": "COMPLETED", "action": "APPROVE",
              "allowedBy": format!("ROLE:{role_code}") },
            { "from": "MANAGER_APPROVAL", "to": "REJECTED", "action": "REJECT",
              "allowedBy": format!("ROLE:{role_code}") },
            { "from": "MANAGER_APPROVAL", "to": "RETURNED", "action": "RETURN",
              "allowedBy": format!("ROLE:{role_code}") },
            { "from": "RETURNED", "to": "MANAGER_APPROVAL", "action": "RESUBMIT",
              "allowedBy": "OWNER" },
            { "from": "MANAGER_APPROVAL", "to": "WITHDRAWN", "action": "CANCEL",
              "allowedBy": "OWNER" }
        ]
    })
}

/// A published workflow of that shape and a type bound to it.
async fn approval_time_type(app: &TestApp, token: &str, code: &str) -> (Uuid, Uuid) {
    let key = code.to_lowercase();
    let workflow =
        publish_workflow_definition(app, token, &key, approval_time_workflow(&key, code)).await;

    (workflow, document_type(app, token, code, workflow).await)
}

/// One `workflow_instances` row, as it stands once a round has ended — or not.
struct Round {
    /// The state the round stopped in; `MANAGER_APPROVAL` while it is live.
    state: &'static str,
    /// `None` while it is live, and on a round that ended without one.
    outcome: Option<&'static str>,
    started: DateTime<Utc>,
    /// `None` while it is live.
    completed: Option<DateTime<Utc>>,
    deleted: bool,
}

impl Round {
    fn ended(state: &'static str, outcome: &'static str, days_ago: (i64, i64)) -> Self {
        let now = Utc::now();

        Self {
            state,
            outcome: Some(outcome),
            started: now - Duration::days(days_ago.0),
            completed: Some(now - Duration::days(days_ago.1)),
            deleted: false,
        }
    }

    fn approved(days_ago: (i64, i64)) -> Self {
        Self::ended("COMPLETED", "APPROVED", days_ago)
    }

    fn live(started_days_ago: i64) -> Self {
        Self {
            state: "MANAGER_APPROVAL",
            outcome: None,
            started: Utc::now() - Duration::days(started_days_ago),
            completed: None,
            deleted: false,
        }
    }
}

/// Writes one instance directly.
///
/// **Planted rather than driven, for the reason [`plant_document`] is**: a
/// decision ninety-one days old, a round in another tenant, an instance on a
/// soft-deleted document and a document with two instances are all shapes a
/// dropped predicate would expose and no API call produces today.
/// [`a_returned_and_resubmitted_document_is_timed_from_its_first_submission`]
/// is the one test here whose rows the engine wrote.
///
/// **The timestamps are bound from one Rust clock**, so the time the statement
/// computes between them is exact to the second and the assertions can be
/// equalities. Only the window is read against the database's `now()`, and every
/// fixture keeps a day's margin from its edge.
async fn plant_round(
    app: &TestApp,
    tenant_id: Uuid,
    document: Uuid,
    workflow: Uuid,
    round: Round,
) -> Uuid {
    let id = Uuid::now_v7();
    let status = if round.completed.is_some() {
        "COMPLETED"
    } else {
        "RUNNING"
    };

    sqlx::query(
        "INSERT INTO workflow_instances (id, tenant_id, instance_ref, workflow_definition_id, \
         document_id, status, current_state, outcome, started_at, completed_at, deleted_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, CASE WHEN $11 THEN now() END)",
    )
    .bind(id)
    .bind(tenant_id)
    .bind(format!("WFI-AT-{id}"))
    .bind(workflow)
    .bind(document)
    .bind(status)
    .bind(round.state)
    .bind(round.outcome)
    .bind(round.started)
    .bind(round.completed)
    .bind(round.deleted)
    .execute(&app.pool)
    .await
    .expect("plant a workflow instance");

    id
}

/// A document of the caller's in `tenant_id`, with the rounds given.
async fn decided_document(
    app: &TestApp,
    tenant_id: Uuid,
    author: Option<Uuid>,
    (workflow, type_id): (Uuid, Uuid),
    rounds: Vec<Round>,
) -> Uuid {
    let document = plant_document(app, tenant_id, author, type_id, "COMPLETED", false).await;

    for round in rounds {
        plant_round(app, tenant_id, document, workflow, round).await;
    }

    document
}

/// `approvalTime` as `(documents, medianSeconds, slowestSeconds)`, after
/// asserting the window it was read over.
fn approval_time(summary: &Value) -> (i64, Option<i64>, Option<i64>) {
    let card = &summary["approvalTime"];

    assert_eq!(
        card["windowDays"], 90,
        "the card was not read over D-83's ninety days: {summary}"
    );

    (
        card["documents"]
            .as_i64()
            .unwrap_or_else(|| panic!("the card carries a count: {summary}")),
        card["medianSeconds"].as_i64(),
        card["slowestSeconds"].as_i64(),
    )
}

/// **The median, the slowest and the count of the caller's decided documents**
/// ([#461] AC1).
///
/// Three documents taking 1, 2 and 10 days, planted in neither ascending nor
/// descending order, **one of them rejected** — a rejection is a decision, and
/// a statement counting approvals alone reads two documents with a median of
/// five and a half days.
///
/// [#461]: https://github.com/sujanto-gaws/kelir/issues/461
#[tokio::test]
async fn the_approval_time_is_the_median_and_the_slowest_of_the_callers_decided_documents() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let shape = approval_time_type(&app, &token, "RD_T_SUM").await;
    let (author_id, author) = holder_party(&app, "RD-T-SUM", "rd.t.sum", &[]).await;
    let tenant = fixtures::SYSTEM_TENANT_ID;

    decided_document(
        &app,
        tenant,
        Some(author_id),
        shape,
        vec![Round::approved((12, 2))],
    )
    .await;
    decided_document(
        &app,
        tenant,
        Some(author_id),
        shape,
        vec![Round::ended("REJECTED", "REJECTED", (3, 1))],
    )
    .await;
    decided_document(
        &app,
        tenant,
        Some(author_id),
        shape,
        vec![Round::approved((4, 3))],
    )
    .await;

    assert_eq!(
        approval_time(&summary_of(&app, &author).await),
        (3, Some(2 * DAY), Some(10 * DAY)),
        "the approval time is not the median and slowest of three decided documents"
    );
}

/// **A document sent back and resubmitted is timed from the first time it was
/// sent** ([#461] AC2) — through the engine, so the rows are the product's.
///
/// # The shape the product runs a return on
///
/// A `RETURN` into a state that is not final **leaves the instance running**,
/// and the resubmission moves that same instance on
/// (`document::service::submit`'s `resubmit_workflow`). So there is one
/// instance, and the test asserts that before trusting anything else: the
/// time starts from *its* `started_at`, and the thing that must not happen is
/// the resubmission restarting the clock.
///
/// # How a round lasts longer than the test
///
/// The first submission is **dated three days back** straight after it is made,
/// with an `UPDATE` — the way [`late_by`] dates a task. Everything after it —
/// the return, the resubmission, the approval — is the engine's own, happening
/// now. A card timing from the resubmission reads seconds; one timing from the
/// first submission reads three days and the seconds the rest took.
///
/// [#461]: https://github.com/sujanto-gaws/kelir/issues/461
#[tokio::test]
async fn a_returned_and_resubmitted_document_is_timed_from_its_first_submission() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let (_, type_id) = approval_time_type(&app, &token, "RD_T_BACK").await;
    let approver = holder(&app, "RD_T_BACK", "rd.t.back.approver").await;
    let (_, author) = holder_party(&app, "RD-T-BACK-AUTHOR", "rd.t.back", &[]).await;

    let document = submitted_document(&app, &author, type_id, "Sent back once").await;

    let dated = sqlx::query(
        "UPDATE workflow_instances SET started_at = now() - interval '3 days' \
         WHERE document_id = $1",
    )
    .bind(document)
    .execute(&app.pool)
    .await
    .expect("date the first submission");
    assert_eq!(
        dated.rows_affected(),
        1,
        "the submission started no instance"
    );

    let decide = |task: String, action: &'static str| {
        let app = &app;
        let approver = &approver;
        async move {
            let decided = app
                .post(
                    &format!("/api/v1/workflow/tasks/{task}/decision"),
                    Some(approver),
                    json!({ "action": action, "comment": "Round decided." }),
                )
                .await;
            assert_eq!(decided.status, StatusCode::OK, "{}", decided.body);
        }
    };

    decide(open_task_of(&app, document).await, "RETURN").await;

    assert_eq!(
        approval_time(&summary_of(&app, &author).await).0,
        0,
        "a document sent back to its author was timed as decided"
    );

    let resubmitted = app
        .send(
            Method::POST,
            &format!("/api/v1/documents/{document}/submission"),
            Some(&author),
            None,
        )
        .await;
    assert_eq!(resubmitted.status, StatusCode::OK, "{}", resubmitted.body);

    decide(open_task_of(&app, document).await, "APPROVE").await;

    let (instances, started_days_ago, took): (i64, f64, i64) = sqlx::query_as(
        "SELECT count(*), \
                EXTRACT(EPOCH FROM now() - min(started_at))::float8 / 86400, \
                EXTRACT(EPOCH FROM (max(completed_at) - min(started_at)))::bigint \
         FROM workflow_instances WHERE document_id = $1",
    )
    .bind(document)
    .fetch_one(&app.pool)
    .await
    .expect("read the document's rounds");

    assert_eq!(
        instances, 1,
        "the resubmission started a second instance, which is not the shape this test is about"
    );
    assert!(
        started_days_ago > 2.99,
        "the resubmission restarted the instance's clock: it started {started_days_ago} days ago"
    );

    assert_eq!(
        approval_time(&summary_of(&app, &author).await),
        (1, Some(took), Some(took)),
        "the returned document was not timed from its first submission"
    );
    assert!(
        took >= 3 * DAY,
        "timed from the resubmission: {took} seconds"
    );
}

/// **A document decided over two instances is timed from the first**
/// ([#461] AC2).
///
/// The other shape a return can take: a definition whose return state is final
/// ends the instance `RETURNED`, and the next submission starts another. Round
/// one runs from ten days ago to eight; round two from five days ago to one day
/// ago, approved. **Nine days**, not the four the deciding instance lasted and
/// not the six the two rounds spent open.
///
/// [#461]: https://github.com/sujanto-gaws/kelir/issues/461
#[tokio::test]
async fn a_document_decided_over_two_instances_is_timed_from_the_first() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let shape = approval_time_type(&app, &token, "RD_T_TWO").await;
    let (author_id, author) = holder_party(&app, "RD-T-TWO", "rd.t.two", &[]).await;

    decided_document(
        &app,
        fixtures::SYSTEM_TENANT_ID,
        Some(author_id),
        shape,
        vec![
            Round::ended("RETURNED", "RETURNED", (10, 8)),
            Round::approved((5, 1)),
        ],
    )
    .await;

    assert_eq!(
        approval_time(&summary_of(&app, &author).await),
        (1, Some(9 * DAY), Some(9 * DAY)),
        "a document with two rounds was not timed from the first one's start"
    );
}

/// **A document still in flight is not timed** ([#461] AC3).
///
/// Two of them: one on its first round, and one **whose earlier round was
/// rejected** and which is running again. The second is the case a statement
/// looking for *any* decided instance would count; it is decided only when its
/// latest instance says so. A third document, decided, keeps the count at one
/// rather than zero, so a card that timed nothing does not pass.
///
/// [#461]: https://github.com/sujanto-gaws/kelir/issues/461
#[tokio::test]
async fn a_document_still_in_flight_is_not_timed() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let shape = approval_time_type(&app, &token, "RD_T_FLY").await;
    let (author_id, author) = holder_party(&app, "RD-T-FLY", "rd.t.fly", &[]).await;
    let tenant = fixtures::SYSTEM_TENANT_ID;

    decided_document(&app, tenant, Some(author_id), shape, vec![Round::live(4)]).await;
    decided_document(
        &app,
        tenant,
        Some(author_id),
        shape,
        vec![
            Round::ended("REJECTED", "REJECTED", (20, 15)),
            Round::live(6),
        ],
    )
    .await;
    decided_document(
        &app,
        tenant,
        Some(author_id),
        shape,
        vec![Round::approved((3, 1))],
    )
    .await;

    assert_eq!(
        approval_time(&summary_of(&app, &author).await),
        (1, Some(2 * DAY), Some(2 * DAY)),
        "a document still in flight was timed"
    );
}

/// **A document whose last instance ended `CANCELLED` is not timed**
/// ([#461] AC3).
///
/// Withdrawn, not decided — and, as with the in-flight test, twice: once on its
/// only round, and once **after an earlier round was rejected**, which a
/// statement reading any decided instance would time at fifteen days.
///
/// [#461]: https://github.com/sujanto-gaws/kelir/issues/461
#[tokio::test]
async fn a_document_whose_last_instance_was_cancelled_is_not_timed() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let shape = approval_time_type(&app, &token, "RD_T_OFF").await;
    let (author_id, author) = holder_party(&app, "RD-T-OFF", "rd.t.off", &[]).await;
    let tenant = fixtures::SYSTEM_TENANT_ID;

    decided_document(
        &app,
        tenant,
        Some(author_id),
        shape,
        vec![Round::ended("WITHDRAWN", "CANCELLED", (6, 2))],
    )
    .await;
    decided_document(
        &app,
        tenant,
        Some(author_id),
        shape,
        vec![
            Round::ended("REJECTED", "REJECTED", (20, 15)),
            Round::ended("WITHDRAWN", "CANCELLED", (10, 5)),
        ],
    )
    .await;
    decided_document(
        &app,
        tenant,
        Some(author_id),
        shape,
        vec![Round::approved((3, 1))],
    )
    .await;

    assert_eq!(
        approval_time(&summary_of(&app, &author).await),
        (1, Some(2 * DAY), Some(2 * DAY)),
        "a withdrawn document was timed as decided"
    );
}

/// **A decision older than ninety days is not timed** ([#461] AC3).
///
/// One decided ninety-one days ago and one eighty-nine, a day either side of
/// the edge, so the database's clock and this test's can disagree by less than
/// a day and change nothing. The old one took four days and the recent one
/// one, so a window read the wrong way round is a different number rather than
/// the same count.
///
/// [#461]: https://github.com/sujanto-gaws/kelir/issues/461
#[tokio::test]
async fn a_decision_older_than_ninety_days_is_not_timed() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let shape = approval_time_type(&app, &token, "RD_T_OLD").await;
    let (author_id, author) = holder_party(&app, "RD-T-OLD", "rd.t.old", &[]).await;
    let tenant = fixtures::SYSTEM_TENANT_ID;

    decided_document(
        &app,
        tenant,
        Some(author_id),
        shape,
        vec![Round::approved((95, 91))],
    )
    .await;
    decided_document(
        &app,
        tenant,
        Some(author_id),
        shape,
        vec![Round::approved((90, 89))],
    )
    .await;

    assert_eq!(
        approval_time(&summary_of(&app, &author).await),
        (1, Some(DAY), Some(DAY)),
        "a decision older than the window was timed, or a recent one was not"
    );
}

/// **Another author's documents are not timed for the caller** ([#461] AC3).
///
/// Two authors in one tenant, asserted both ways round (coding standard §2.9),
/// and a third document **nobody raised** — `created_by` is nullable, and a
/// looser *the caller's, or nobody's* would time every system-raised document on
/// every dashboard.
///
/// [#461]: https://github.com/sujanto-gaws/kelir/issues/461
#[tokio::test]
async fn another_authors_documents_are_not_timed_for_the_caller() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let shape = approval_time_type(&app, &token, "RD_T_WHO").await;
    let (ani_id, ani) = holder_party(&app, "RD-T-ANI", "rd.t.ani", &[]).await;
    let (budi_id, budi) = holder_party(&app, "RD-T-BUDI", "rd.t.budi", &[]).await;
    let tenant = fixtures::SYSTEM_TENANT_ID;

    decided_document(
        &app,
        tenant,
        Some(ani_id),
        shape,
        vec![Round::approved((2, 1))],
    )
    .await;
    decided_document(
        &app,
        tenant,
        Some(budi_id),
        shape,
        vec![Round::approved((8, 1))],
    )
    .await;
    decided_document(&app, tenant, None, shape, vec![Round::approved((30, 1))]).await;

    assert_eq!(
        approval_time(&summary_of(&app, &ani).await),
        (1, Some(DAY), Some(DAY)),
        "Ani's approval time included somebody else's document"
    );
    assert_eq!(
        approval_time(&summary_of(&app, &budi).await),
        (1, Some(7 * DAY), Some(7 * DAY)),
        "Budi's approval time lost his own document or gained another"
    );
}

/// **A second tenant's documents are not timed here** ([#461] AC3).
///
/// Planted under another tenant — the document and its instance both — **with
/// this caller as the author**, because that is the only row a dropped tenant
/// predicate would expose: anything raised through the API is stamped with the
/// caller's own tenant.
///
/// [#461]: https://github.com/sujanto-gaws/kelir/issues/461
#[tokio::test]
async fn a_second_tenants_documents_are_not_timed_here() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let shape = approval_time_type(&app, &token, "RD_T_TEN").await;
    let (author_id, author) = holder_party(&app, "RD-T-TEN", "rd.t.ten", &[]).await;
    let other_tenant = fixtures::create_tenant(&app.pool, "RD-T-OTHER", "Other tenant").await;

    decided_document(
        &app,
        fixtures::SYSTEM_TENANT_ID,
        Some(author_id),
        shape,
        vec![Round::approved((2, 1))],
    )
    .await;
    decided_document(
        &app,
        other_tenant,
        Some(author_id),
        shape,
        vec![Round::approved((40, 1))],
    )
    .await;

    assert_eq!(
        approval_time(&summary_of(&app, &author).await),
        (1, Some(DAY), Some(DAY)),
        "a document filed under another tenant was timed on this caller's dashboard"
    );
}

/// **An instance stamped with this tenant cannot bring another tenant's
/// document onto the card** ([#461] AC3).
///
/// `workflow_instances.document_id` references `documents (id)` alone, so the
/// schema allows an instance whose tenant is not its document's. That row
/// **passes the instance's tenant predicate** — it is this tenant's — and only
/// carrying the tenant across the join refuses the document on the other end.
/// [`a_second_tenants_documents_are_not_timed_here`] cannot see that half: its
/// foreign instance is foreign too, and the instance's own predicate drops it
/// first. This is the FR-RPT-003 table's M3 finding, written before the
/// mutation rather than after it.
///
/// [#461]: https://github.com/sujanto-gaws/kelir/issues/461
#[tokio::test]
async fn another_tenants_document_is_not_timed_through_an_instance_filed_here() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let (workflow, type_id) = approval_time_type(&app, &token, "RD_T_MIX").await;
    let (author_id, author) = holder_party(&app, "RD-T-MIX", "rd.t.mix", &[]).await;
    let tenant = fixtures::SYSTEM_TENANT_ID;
    let other_tenant = fixtures::create_tenant(&app.pool, "RD-T-MIX-OTHER", "Other tenant").await;

    decided_document(
        &app,
        tenant,
        Some(author_id),
        (workflow, type_id),
        vec![Round::approved((2, 1))],
    )
    .await;

    let foreign = plant_document(
        &app,
        other_tenant,
        Some(author_id),
        type_id,
        "COMPLETED",
        false,
    )
    .await;
    plant_round(&app, tenant, foreign, workflow, Round::approved((40, 1))).await;

    assert_eq!(
        approval_time(&summary_of(&app, &author).await),
        (1, Some(DAY), Some(DAY)),
        "an instance in this tenant timed a document filed under another"
    );
}

/// **A soft-deleted document is not timed** ([#461] AC3).
///
/// Two decided documents, one deleted, so the assertion is *one* rather than
/// *none*.
///
/// [#461]: https://github.com/sujanto-gaws/kelir/issues/461
#[tokio::test]
async fn a_soft_deleted_document_is_not_timed() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let (workflow, type_id) = approval_time_type(&app, &token, "RD_T_GONE").await;
    let (author_id, author) = holder_party(&app, "RD-T-GONE", "rd.t.gone", &[]).await;
    let tenant = fixtures::SYSTEM_TENANT_ID;

    decided_document(
        &app,
        tenant,
        Some(author_id),
        (workflow, type_id),
        vec![Round::approved((2, 1))],
    )
    .await;

    let deleted = plant_document(&app, tenant, Some(author_id), type_id, "COMPLETED", true).await;
    plant_round(&app, tenant, deleted, workflow, Round::approved((20, 1))).await;

    assert_eq!(
        approval_time(&summary_of(&app, &author).await),
        (1, Some(DAY), Some(DAY)),
        "a soft-deleted document was timed"
    );
}

/// **A soft-deleted instance does not start a document's clock.**
///
/// The document is live and decided in one day. A deleted round from thirty
/// days before is planted beside that one, so a statement that read it would
/// both see an earlier start — twenty-nine days — and not merely one more row.
///
/// [#461]: https://github.com/sujanto-gaws/kelir/issues/461
#[tokio::test]
async fn a_soft_deleted_instance_is_not_timed() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let shape = approval_time_type(&app, &token, "RD_T_UNDO").await;
    let (author_id, author) = holder_party(&app, "RD-T-UNDO", "rd.t.undo", &[]).await;

    let mut deleted_round = Round::ended("RETURNED", "RETURNED", (30, 29));
    deleted_round.deleted = true;

    decided_document(
        &app,
        fixtures::SYSTEM_TENANT_ID,
        Some(author_id),
        shape,
        vec![deleted_round, Round::approved((2, 1))],
    )
    .await;

    assert_eq!(
        approval_time(&summary_of(&app, &author).await),
        (1, Some(DAY), Some(DAY)),
        "a soft-deleted instance moved the document's start"
    );
}

/// **Nothing decided is a zero and no times, not zero seconds** ([#461] AC1).
///
/// The caller has a document in flight and nothing else. `null` is what lets
/// the card say *nothing decided yet* rather than *decided instantly*.
///
/// [#461]: https://github.com/sujanto-gaws/kelir/issues/461
#[tokio::test]
async fn a_caller_with_nothing_decided_has_nothing_timed() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let shape = approval_time_type(&app, &token, "RD_T_NONE").await;
    let (author_id, author) = holder_party(&app, "RD-T-NONE", "rd.t.none", &[]).await;

    decided_document(
        &app,
        fixtures::SYSTEM_TENANT_ID,
        Some(author_id),
        shape,
        vec![Round::live(3)],
    )
    .await;

    let summary = summary_of(&app, &author).await;

    assert_eq!(approval_time(&summary), (0, None, None), "{summary}");
    assert!(
        summary["approvalTime"]["medianSeconds"].is_null()
            && summary["approvalTime"]["slowestSeconds"].is_null(),
        "nothing decided was served as a time rather than null: {summary}"
    );
}

/// **The card needs the dashboard grant and nothing else** ([#461] AC1,
/// **D-82**'s invariant).
///
/// The caller holds `reporting:dashboard:read` alone — not
/// `workflow:instance:read`, not `document:read` — and is still timed, because
/// every number on the card is about documents they raised.
///
/// [#461]: https://github.com/sujanto-gaws/kelir/issues/461
#[tokio::test]
async fn the_approval_time_is_timed_under_the_dashboard_grant_alone() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let shape = approval_time_type(&app, &token, "RD_T_ONLY").await;

    let role = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "RD-T-ONLY",
        &["reporting:dashboard:read"],
    )
    .await;
    let author_id = fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "rd.t.only",
        "rd.t.only@example.test",
        common::ADMIN_PASSWORD,
        &[role],
    )
    .await;
    let author = app.sign_in("rd.t.only", common::ADMIN_PASSWORD).await;

    decided_document(
        &app,
        fixtures::SYSTEM_TENANT_ID,
        Some(author_id),
        shape,
        vec![Round::approved((6, 1))],
    )
    .await;

    assert_eq!(
        approval_time(&summary_of(&app, &author).await),
        (1, Some(5 * DAY), Some(5 * DAY)),
        "the approval time asked for a grant beyond the dashboard's"
    );
}

/// **The card is in the published contract, with its four fields**
/// ([#461] AC1) — and there is still one dashboard path.
///
/// `ApprovalTime` is registered in `router.rs`'s `components(schemas(...))`
/// separately from its `ToSchema` derive, so forgetting it leaves a dangling
/// `$ref`. The field names are asserted, camelCase, because a generated client
/// reads them from here and not from the wire.
///
/// [#461]: https://github.com/sujanto-gaws/kelir/issues/461
#[tokio::test]
async fn the_approval_time_is_in_the_published_contract() {
    let app = TestApp::spawn().await;

    let document = app.get("/api/docs/openapi.json", None).await;
    assert_eq!(document.status, StatusCode::OK);

    let schemas = &document.body["components"]["schemas"];

    assert!(
        !schemas["DashboardSummary"]["properties"]["approvalTime"].is_null(),
        "DashboardSummary does not carry approvalTime"
    );

    let properties = schemas["ApprovalTime"]["properties"]
        .as_object()
        .unwrap_or_else(|| {
            panic!("approvalTime references ApprovalTime and it is not in the contract")
        });
    let mut fields: Vec<&str> = properties.keys().map(String::as_str).collect();
    fields.sort_unstable();

    assert_eq!(
        fields,
        ["documents", "medianSeconds", "slowestSeconds", "windowDays"],
        "ApprovalTime's fields"
    );

    let paths = document.body["paths"]
        .as_object()
        .expect("the contract has paths");
    let reporting: Vec<&String> = paths
        .keys()
        .filter(|path| path.contains("/dashboard") || path.contains("/report"))
        .collect();

    assert_eq!(
        reporting,
        vec!["/api/v1/dashboard/summary"],
        "the approval time arrived as an endpoint beside the summary (ADR-0039)"
    );
}
