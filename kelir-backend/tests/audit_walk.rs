//! **Every mutating operation writes its audit row, but the ones ruled exempt**
//! (NFR-AUD-001 to 004, [#960], construction plan 22 row 15).
//!
//! `no_token_walk.rs` walks every operation in `ApiDoc::openapi()` for its
//! caller. This walks every **mutating** one for its trail: each `POST`, `PUT`
//! and `DELETE` the document holds is sent once, as a request that succeeds,
//! and must leave an `audit_events` row naming its actor, its time, its action
//! and its object. It is read from the document, so a route added tomorrow is
//! walked tomorrow, or fails at a door.
//!
//! # The two tables, and the doors
//!
//! - [`CELLS`] lists the operations that write an audit row. [`setup`] holds
//!   each one's cell: it creates the cell's subject through the API, and says
//!   which request to send, as whom, and which row it must write.
//! - [`EXEMPT`] lists the operations that write none, each with its reason and
//!   its ruling. **The product owner ruled all four on 2026-11-16** on #960.
//!
//! The doors, held by [`door_failures`] before anything is sent:
//! - every mutating operation in the document is in exactly one of the two;
//! - every entry in either names an operation the document has, and a
//!   mutating one;
//! - so a new mutating route that is in neither fails here, by name.
//!
//! `no_token_walk.rs`'s `every_served_method_is_in_the_document` holds the
//! document equal to what `src/` serves, so a route left out of the document
//! cannot stand outside this walk either.
//!
//! # What a cell asserts
//!
//! The audit trail's high-water mark is read right before the request, and the
//! cell fails unless:
//! - the request answers 2xx (a refusal is a failed cell: an action that did
//!   not happen proves nothing about the row it writes);
//! - at least one new row has the cell's `event_type`, `object_type` and
//!   `object_id`;
//! - that row has a non-empty `action`, a `created_at` inside the request's
//!   window, and the caller as its actor, or no actor where the cell says so.
//!
//! The cells run one at a time, and a failed cell does not stop the walk: every
//! failure is listed at the end. Each exempt operation is sent too, must answer
//! 2xx, and must write **no** row: a row that is now written is an exemption to
//! delete. Last, the whole chain is verified with the verifier
//! `audit_hash_chain.rs` uses ([`common::audit_chain`]).
//!
//! **Role ids are live ones** ([#996]): a user is given a role that exists, so
//! a cell cannot answer 500 for the foreign key.
//!
//! # What a cell does not assert, and where it is
//!
//! **Content.** A cell asserts that the row exists and whom it names, not what
//! its `old_value` and `new_value` say. Two rows are known to say the wrong
//! thing, and each is pinned below by a test named for today's behaviour,
//! asserting the exact wrong value. **Each passes today. The row that fixes it
//! inverts the test and renames it**; it is not `#[ignore]`d, and nothing is
//! weakened to make it pass. Their route cells assert only what is right today.
//!
//! | Finding | Issue | Pinned by |
//! |---|---|---|
//! | A: a role edit audits the request, not the stored row | [#1000] | `a_role_edit_audits_permission_ids_it_was_not_sent_issue_1000` |
//! | B: a tenant edit audits the request, not the stored row | [#1001] | `a_tenant_status_edit_audits_a_name_it_was_not_sent_issue_1001` |
//! | C: a change master-data governance applies writes no row on the record | [#1002] | `a_governed_change_writes_no_audit_row_on_the_party_issue_1002` |
//! | D: a lockout engaging writes no row of its own | [#1003] | `a_lockout_engaging_writes_no_audit_row_of_its_own_issue_1003` |
//! | E: a scan result writes no row | [#1004] | `a_scan_result_writes_no_audit_row_issue_1004` |
//!
//! [#795]'s shape, the request audited for the stored row, was fixed for users
//! by #994 (row 23). A is the same shape on roles.
//!
//! # The mutations that are not routes
//!
//! Eleven, from the count on #960. Each has a named test here, or a ruling:
//!
//! | Mutation | Today | Here |
//! |---|---|---|
//! | The bootstrap administrator | audited, with no actor | `the_bootstrap_administrator_is_audited_with_no_actor` |
//! | Tenant provisioning | audited as `Tenant.Created` | the `POST /api/v1/organization/tenants` cell |
//! | Automatic transitions and system-raised tasks | the triggering decision's row only | `an_automatic_step_writes_no_audit_row_of_its_own_issue_1005` |
//! | The attachment scan worker | nothing | E above |
//! | Master-data governance's raise, and its settle | nothing on the record | C above |
//! | The lockout | nothing of its own | D above |
//! | The notification worker, the outbox worker | nothing; **ruled exempt** | `the_notification_and_outbox_workers_write_no_audit_row_as_ruled` |
//! | The permission grants in migrations | nothing; **ruled exempt** (I): the migration history is their trace | no test: a migration runs before any tenant has a chain to write to, and `_sqlx_migrations` records each one with its checksum |
//!
//! [#1005] (F) is **out of scope for `v1.0.0`** by the same ruling; its test
//! pins what is audited today. [#1006] (G, deletes with no `old_value`) is a
//! *should* and is not asserted here.
//!
//! # AUD-003: an approval read back through the trail
//!
//! `an_approval_is_read_back_from_the_audit_trail_one_decision_at_a_time`
//! drives a submit, a manager's approval and finance's, and reads each decision
//! back through `GET /api/v1/audit` by its `WORKFLOW_TASK` object, cross-checked
//! against `/workflow/history`. Approvals are audited on the task, not the
//! document ([#1005]).
//!
//! # Out of scope
//!
//! [#995]: two audited actions at once can fork a tenant's chain. The walk is
//! sequential, so it neither meets nor proves that; the chain it verifies is a
//! chain written one row at a time.
//!
//! [#960]: https://github.com/sujanto-gaws/kelir/issues/960
//! [#795]: https://github.com/sujanto-gaws/kelir/issues/795
//! [#995]: https://github.com/sujanto-gaws/kelir/issues/995
//! [#996]: https://github.com/sujanto-gaws/kelir/issues/996
//! [#1000]: https://github.com/sujanto-gaws/kelir/issues/1000
//! [#1001]: https://github.com/sujanto-gaws/kelir/issues/1001
//! [#1002]: https://github.com/sujanto-gaws/kelir/issues/1002
//! [#1003]: https://github.com/sujanto-gaws/kelir/issues/1003
//! [#1004]: https://github.com/sujanto-gaws/kelir/issues/1004
//! [#1005]: https://github.com/sujanto-gaws/kelir/issues/1005
//! [#1006]: https://github.com/sujanto-gaws/kelir/issues/1006

mod common;

use std::collections::{BTreeSet, HashSet};
use std::net::SocketAddr;
use std::panic::AssertUnwindSafe;

use axum::http::{Method, StatusCode};
use chrono::{DateTime, Utc};
use futures_util::FutureExt;
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use uuid::Uuid;

use common::audit_chain::{chain_breaks, stored_rows, StoredRow};
use common::fixtures::{self, SYSTEM_TENANT_ID};
use common::operations::{every_operation, Operation};
use common::{TestApp, TestResponse, ADMIN_USERNAME};
use kelir_backend::modules::attachment::worker;

/// Every fixture user's password.
const PASSWORD: &str = "a-walk-password-long-enough";

/// The password a cell changes one to.
const NEW_PASSWORD: &str = "a-changed-walk-password";

/// A real PDF header: the upload's type check reads the bytes.
const PDF: &[u8] = b"%PDF-1.7\n1 0 obj\n<< /Type /Catalog >>\nendobj\ntrailer\n<<>>\n%%EOF\n";

/// The `QUOTATION` category `0037_attachment_tail.sql` seeds for the system
/// tenant.
const QUOTATION: &str = "00000000-0000-0000-0003-000000000001";

/// The role every workflow here offers its task to.
const APPROVER_ROLE: &str = "WALK-APPROVER";

/// What a holder of [`APPROVER_ROLE`] needs: the task, its document, and the
/// notification centre the exempt mark-read routes are in.
const APPROVER_PERMISSIONS: [&str; 5] = [
    "workflow:task:read",
    "workflow:task:execute",
    "workflow:instance:read",
    "document:read",
    "notification:read",
];

const RULED: &str = "product owner, 2026-11-16, #960";

/// **Every mutating operation that writes no audit row, and why.** A row here
/// is a ruling that the trail need not record the operation; review it as one.
/// Each is sent by the walk, and must answer 2xx and write no row.
const EXEMPT: [(&str, &str, &str); 4] = [
    (
        "POST /api/v1/auth/refresh",
        "rotates the session a sign-in opened, and that sign-in is audited as \
         Security.SignedIn; a reused token is audited as Security.RefreshTokenReused",
        RULED,
    ),
    (
        "POST /api/v1/notifications/read",
        "marks the caller's own notifications read: a reading state of the \
         caller's inbox, which changes no record anybody else sees",
        RULED,
    ),
    (
        "POST /api/v1/notifications/{id}/read",
        "marks one of the caller's own notifications read, for the same reason",
        RULED,
    ),
    (
        "POST /api/v1/integration/external-systems/{id}/endpoints/{endpointId}/test-call",
        "changes no record; integration_logs records each call, its caller and \
         its outcome",
        RULED,
    ),
];

/// **Every mutating operation that writes an audit row.** [`setup`] holds each
/// one's cell, and a name here with no cell there fails the walk.
const CELLS: [&str; 78] = [
    // Authentication.
    "POST /api/v1/auth/login",
    "POST /api/v1/auth/logout",
    "POST /api/v1/auth/change-password",
    "POST /api/v1/auth/forgot-password",
    "POST /api/v1/auth/reset-password",
    // Document types.
    "POST /api/v1/document-types",
    "PUT /api/v1/document-types/{id}",
    "DELETE /api/v1/document-types/{id}",
    "PUT /api/v1/document-types/{id}/numbering-rule",
    "DELETE /api/v1/document-types/{id}/numbering-rule",
    // Documents, and what hangs on them.
    "POST /api/v1/documents",
    "PUT /api/v1/documents/{id}",
    "DELETE /api/v1/documents/{id}",
    "POST /api/v1/documents/{id}/submission",
    "PUT /api/v1/documents/{id}/status",
    "POST /api/v1/documents/{id}/attachments",
    "DELETE /api/v1/documents/{id}/attachments/{attachment_id}",
    "POST /api/v1/documents/{id}/comments",
    "PUT /api/v1/documents/{id}/comments/{comment_id}",
    "DELETE /api/v1/documents/{id}/comments/{comment_id}",
    "POST /api/v1/documents/{id}/references",
    "DELETE /api/v1/documents/{id}/references/{reference_id}",
    // Identity.
    "POST /api/v1/identity/users",
    "PUT /api/v1/identity/users/{id}",
    "DELETE /api/v1/identity/users/{id}",
    "POST /api/v1/identity/users/{id}/password",
    "POST /api/v1/identity/roles",
    "PUT /api/v1/identity/roles/{id}",
    "DELETE /api/v1/identity/roles/{id}",
    "POST /api/v1/identity/delegations",
    "DELETE /api/v1/identity/delegations/{id}",
    // Integration.
    "POST /api/v1/integration/external-systems",
    "PUT /api/v1/integration/external-systems/{id}",
    "POST /api/v1/integration/external-systems/{id}/deactivate",
    "POST /api/v1/integration/external-systems/{id}/activate",
    "POST /api/v1/integration/external-systems/{id}/endpoints",
    "PUT /api/v1/integration/external-systems/{id}/endpoints/{endpointId}",
    "POST /api/v1/integration/external-systems/{id}/credentials",
    "PUT /api/v1/integration/external-systems/{id}/credentials/{credentialId}",
    "DELETE /api/v1/integration/external-systems/{id}/credentials/{credentialId}",
    // Master data (NFR-AUD-004).
    "POST /api/v1/master-data/facilities",
    "PUT /api/v1/master-data/facilities/{id}",
    "DELETE /api/v1/master-data/facilities/{id}",
    "POST /api/v1/master-data/facilities/{id}/transition",
    "POST /api/v1/master-data/parties",
    "PUT /api/v1/master-data/parties/{id}",
    "DELETE /api/v1/master-data/parties/{id}",
    "PUT /api/v1/master-data/parties/{id}/roles/{roleTypeId}",
    "DELETE /api/v1/master-data/parties/{id}/roles/{roleTypeId}",
    "POST /api/v1/master-data/parties/{id}/transition",
    // Organization.
    "POST /api/v1/organization/departments",
    "PUT /api/v1/organization/departments/{id}",
    "DELETE /api/v1/organization/departments/{id}",
    "POST /api/v1/organization/tenants",
    "PUT /api/v1/organization/tenants/{id}",
    "DELETE /api/v1/organization/tenants/{id}",
    // RAD.
    "POST /api/v1/rad/forms",
    "PUT /api/v1/rad/forms/{id}",
    "DELETE /api/v1/rad/forms/{id}",
    "POST /api/v1/rad/forms/{id}/publish",
    "POST /api/v1/rad/forms/{id}/revisions",
    "POST /api/v1/rad/forms/{id}/submissions",
    "POST /api/v1/rad/lists",
    "PUT /api/v1/rad/lists/{id}",
    "DELETE /api/v1/rad/lists/{id}",
    "POST /api/v1/rad/menus",
    "PUT /api/v1/rad/menus/{id}",
    "DELETE /api/v1/rad/menus/{id}",
    // Workflow.
    "POST /api/v1/workflow/definitions",
    "PUT /api/v1/workflow/definitions/{id}",
    "DELETE /api/v1/workflow/definitions/{id}",
    "POST /api/v1/workflow/definitions/{id}/publication",
    "POST /api/v1/workflow/definitions/{id}/deprecation",
    "POST /api/v1/workflow/definitions/{id}/revisions",
    "POST /api/v1/workflow/tasks/{id}/claim",
    "POST /api/v1/workflow/tasks/{id}/decision",
    "POST /api/v1/workflow/tasks/{id}/delegation",
    "POST /api/v1/workflow/tasks/{id}/reassign",
];

// ---------------------------------------------------------------------------
// The doors
// ---------------------------------------------------------------------------

/// What keeps the document, [`CELLS`] and [`EXEMPT`] from disagreeing: each
/// failure names its row.
fn door_failures(operations: &[Operation]) -> Vec<String> {
    let documented: BTreeSet<String> = operations.iter().map(Operation::name).collect();
    let mutating: BTreeSet<String> = operations
        .iter()
        .filter(|operation| operation.is_mutating())
        .map(Operation::name)
        .collect();
    let cells: BTreeSet<&str> = CELLS.iter().copied().collect();
    let exempt: BTreeSet<&str> = EXEMPT.iter().map(|(name, _, _)| *name).collect();

    let mut failures = Vec::new();

    if cells.len() != CELLS.len() {
        failures.push("CELLS lists an operation more than once".to_owned());
    }
    if exempt.len() != EXEMPT.len() {
        failures.push("EXEMPT lists an operation more than once".to_owned());
    }

    for name in &mutating {
        match (
            cells.contains(name.as_str()),
            exempt.contains(name.as_str()),
        ) {
            (false, false) => failures.push(format!(
                "{name} is a mutating operation in neither CELLS nor EXEMPT: \
                 give it a cell, or a ruling and its reason"
            )),
            (true, true) => failures.push(format!(
                "{name} is in CELLS and in EXEMPT: one of the two is wrong"
            )),
            _ => {}
        }
    }

    for name in cells.iter().chain(exempt.iter()) {
        if !documented.contains(*name) {
            failures.push(format!("{name} is listed, and is not in the document"));
        } else if !mutating.contains(*name) {
            failures.push(format!("{name} is listed, and changes nothing"));
        }
    }

    for (name, reason, ruled) in EXEMPT {
        if reason.trim().is_empty() || ruled.trim().is_empty() {
            failures.push(format!("{name} is exempt with no reason or no ruling"));
        }
    }

    failures
}

/// Whether `path` is `template` with each `{capture}` filled.
fn fills(template: &str, path: &str) -> bool {
    let template: Vec<&str> = template.split('/').collect();
    let path: Vec<&str> = path.split('/').collect();

    template.len() == path.len()
        && template.iter().zip(&path).all(|(expected, actual)| {
            (expected.starts_with('{') && expected.ends_with('}') && !actual.is_empty())
                || expected == actual
        })
}

// ---------------------------------------------------------------------------
// A cell
// ---------------------------------------------------------------------------

/// A request body.
enum Body {
    Empty,
    Json(Value),
    /// A one-page PDF, as `multipart/form-data`.
    Pdf,
}

/// Which object the row must name.
enum Subject {
    /// One the setup created.
    Known(Uuid),
    /// The one the request created: its answer's `data.id`.
    Answered,
}

/// Who sends the request, and whom the row must name as its actor.
struct Caller {
    token: Option<String>,
    actor: Option<Uuid>,
}

/// One cell of the walk.
struct Cell {
    caller: Caller,
    path: String,
    body: Body,
    event_type: &'static str,
    object_type: &'static str,
    object: Subject,
}

fn cell(
    caller: Caller,
    path: String,
    body: Body,
    event_type: &'static str,
    object_type: &'static str,
    object: Subject,
) -> Cell {
    Cell {
        caller,
        path,
        body,
        event_type,
        object_type,
        object,
    }
}

/// The method of a `METHOD template` name, and its template.
fn parts(name: &str) -> (Method, &str) {
    let (method, template) = name.split_once(' ').expect("METHOD template");
    (
        Method::from_bytes(method.as_bytes()).expect("a method"),
        template,
    )
}

async fn send(
    app: &TestApp,
    method: Method,
    path: &str,
    caller: &Caller,
    body: Body,
) -> TestResponse {
    let token = caller.token.as_deref();

    match body {
        Body::Empty => app.send(method, path, token, None).await,
        Body::Json(json) => app.send(method, path, token, Some(json)).await,
        Body::Pdf => {
            app.post_multipart(path, token, "quotation.pdf", "application/pdf", PDF, None)
                .await
        }
    }
}

/// Now, at the resolution `audit_events.created_at` holds.
fn now_in_microseconds() -> DateTime<Utc> {
    let now = Utc::now();
    DateTime::from_timestamp_micros(now.timestamp_micros()).unwrap_or(now)
}

/// The ids of every audit row so far: the high-water mark.
async fn audit_ids(app: &TestApp) -> HashSet<Uuid> {
    let ids: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM audit_events")
        .fetch_all(&app.pool)
        .await
        .expect("the audit ids");
    ids.into_iter().collect()
}

/// The audit rows written since `before` was read, oldest first.
async fn rows_since(app: &TestApp, before: &HashSet<Uuid>) -> Vec<StoredRow> {
    stored_rows(app)
        .await
        .into_iter()
        .filter(|row| !before.contains(&row.id))
        .collect()
}

fn listed(rows: &[StoredRow]) -> String {
    if rows.is_empty() {
        return "nothing".to_owned();
    }

    rows.iter()
        .map(|row| format!("[{} by {:?}]", row.label(), row.actor_user_id))
        .collect::<Vec<_>>()
        .join(", ")
}

fn answered_id(response: &TestResponse) -> Option<Uuid> {
    response.data()["id"]
        .as_str()
        .and_then(|id| id.parse().ok())
}

/// One cell: its setup, its request, and the row it must have written.
async fn walk_cell(walk: &mut Walk, name: &str) -> Result<(), String> {
    let (method, template) = parts(name);

    let Some(cell) = setup(walk, name).await else {
        return Err("CELLS names it and `setup` has no cell for it".to_owned());
    };

    if !fills(template, &cell.path) {
        return Err(format!(
            "the cell sends {}, which is not this operation",
            cell.path
        ));
    }

    let before = audit_ids(&walk.app).await;
    let from = now_in_microseconds();
    let response = send(&walk.app, method, &cell.path, &cell.caller, cell.body).await;
    let to = Utc::now();

    if !response.status.is_success() {
        return Err(format!(
            "refused, so it proves nothing about its row: {} {}",
            response.status, response.body
        ));
    }

    let object_id = match cell.object {
        Subject::Known(id) => id,
        Subject::Answered => answered_id(&response)
            .ok_or_else(|| format!("answered with no data.id: {}", response.body))?,
    };

    let written = rows_since(&walk.app, &before).await;
    let Some(row) = written.iter().find(|row| {
        row.event_type == cell.event_type
            && row.object_type == cell.object_type
            && row.object_id == object_id
    }) else {
        return Err(format!(
            "wrote no {} row on {} {object_id}; it wrote {}",
            cell.event_type,
            cell.object_type,
            listed(&written)
        ));
    };

    let mut wrong = Vec::new();
    if row.action.trim().is_empty() {
        wrong.push("its action is empty".to_owned());
    }
    if !(from..=to).contains(&row.created_at) {
        wrong.push(format!(
            "it was written at {}, outside the request's window {from} to {to}",
            row.created_at
        ));
    }
    if row.actor_user_id != cell.caller.actor {
        wrong.push(format!(
            "its actor is {:?}, and the caller is {:?}",
            row.actor_user_id, cell.caller.actor
        ));
    }

    if wrong.is_empty() {
        Ok(())
    } else {
        Err(format!("{}: {}", row.label(), wrong.join("; ")))
    }
}

/// One exempt operation: sent, answered 2xx, and no row written.
async fn walk_exempt(walk: &mut Walk, name: &str) -> Result<(), String> {
    let (method, template) = parts(name);

    let Some((caller, path, body)) = exempt_setup(walk, name).await else {
        return Err("EXEMPT names it and `exempt_setup` has no request for it".to_owned());
    };

    if !fills(template, &path) {
        return Err(format!(
            "the walk sends {path}, which is not this operation"
        ));
    }

    let before = audit_ids(&walk.app).await;
    let response = send(&walk.app, method, &path, &caller, body).await;

    if !response.status.is_success() {
        return Err(format!(
            "refused, so it proves nothing about what it writes: {} {}",
            response.status, response.body
        ));
    }

    let written = rows_since(&walk.app, &before).await;
    if written.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "is exempt, and wrote {}: delete the exemption and give it a cell",
            listed(&written)
        ))
    }
}

fn panic_text(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| {
            payload
                .downcast_ref::<&str>()
                .map(|text| (*text).to_owned())
        })
        .unwrap_or_else(|| "a panic with no message".to_owned())
}

// ---------------------------------------------------------------------------
// The walk
// ---------------------------------------------------------------------------

/// **The walk.**
#[tokio::test]
async fn every_mutating_operation_writes_its_audit_row_unless_it_is_ruled_exempt() {
    let operations = every_operation();

    let failures = door_failures(&operations);
    assert!(
        failures.is_empty(),
        "the document, CELLS and EXEMPT disagree:\n{}",
        failures.join("\n")
    );

    let mutating = operations
        .iter()
        .filter(|operation| operation.is_mutating())
        .count();
    assert_eq!(
        mutating,
        CELLS.len() + EXEMPT.len(),
        "the doors passed and the tables do not add up to the document"
    );

    let mut walk = Walk::start().await;
    let mut failures = Vec::new();

    for name in CELLS {
        match AssertUnwindSafe(walk_cell(&mut walk, name))
            .catch_unwind()
            .await
        {
            Ok(Ok(())) => {}
            Ok(Err(failure)) => failures.push(format!("{name}: {failure}")),
            Err(panic) => {
                failures.push(format!("{name}: its setup failed: {}", panic_text(&*panic)))
            }
        }
    }

    for (name, _, _) in EXEMPT {
        match AssertUnwindSafe(walk_exempt(&mut walk, name))
            .catch_unwind()
            .await
        {
            Ok(Ok(())) => {}
            Ok(Err(failure)) => failures.push(format!("{name}: {failure}")),
            Err(panic) => {
                failures.push(format!("{name}: its setup failed: {}", panic_text(&*panic)))
            }
        }
    }

    let breaks = chain_breaks(&stored_rows(&walk.app).await);

    assert!(
        failures.is_empty(),
        "{} of {} cells failed:\n{}",
        failures.len(),
        mutating,
        failures.join("\n")
    );
    assert!(
        breaks.is_empty(),
        "the walk's chain does not verify:\n{}",
        breaks.join("\n")
    );
}

/// The walk's application, its administrator, and a serial for unique codes.
struct Walk {
    app: TestApp,
    admin: String,
    admin_id: Uuid,
    approver_role: Uuid,
    /// The base URL of a system that answers `ok` to anything, for the
    /// test-call route.
    mock: String,
    serial: usize,
}

/// The id of a user, by name.
async fn user_id(app: &TestApp, username: &str) -> Uuid {
    sqlx::query_scalar("SELECT id FROM users WHERE username = $1 AND deleted_at IS NULL")
        .bind(username)
        .fetch_one(&app.pool)
        .await
        .expect("the user")
}

fn id_of(data: &Value) -> Uuid {
    data["id"]
        .as_str()
        .and_then(|id| id.parse().ok())
        .unwrap_or_else(|| panic!("no id in {data}"))
}

/// A system that answers 200 `ok` to every request.
async fn mock_system() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind the mock system");
    let address: SocketAddr = listener.local_addr().expect("its address");

    tokio::spawn(async move {
        let router = axum::Router::new().fallback(|| async { "ok" });
        axum::serve(listener, router)
            .await
            .expect("the mock serves");
    });

    format!("http://{address}")
}

/// A one-task approval offered to [`APPROVER_ROLE`].
fn approval(key: &str) -> Value {
    json!({
        "workflowKey": key,
        "version": "1.0.0",
        "name": "Standard approval",
        "initialState": "MANAGER_APPROVAL",
        "states": [
            { "code": "MANAGER_APPROVAL", "name": "Manager approval",
              "mapsToDocumentStatus": "PENDING_APPROVAL",
              "task": { "taskDefinitionKey": "manager_approval", "taskName": "Approve the request",
                        "assignment": { "assigneeType": "ROLE", "roleCode": APPROVER_ROLE } } },
            { "code": "COMPLETED", "name": "Completed", "mapsToDocumentStatus": "COMPLETED",
              "isFinal": true },
            { "code": "REJECTED", "name": "Rejected", "mapsToDocumentStatus": "REJECTED",
              "isFinal": true }
        ],
        "transitions": [
            { "from": "MANAGER_APPROVAL", "to": "COMPLETED", "action": "APPROVE",
              "allowedBy": format!("ROLE:{APPROVER_ROLE}") },
            { "from": "MANAGER_APPROVAL", "to": "REJECTED", "action": "REJECT",
              "allowedBy": format!("ROLE:{APPROVER_ROLE}") }
        ],
        "variables": [
            { "key": "amount", "dataType": "NUMBER", "source": { "var": "formData.amount" } }
        ]
    })
}

/// The open task of a document.
async fn open_task(app: &TestApp, document: Uuid) -> Uuid {
    sqlx::query_scalar(
        "SELECT id FROM workflow_tasks WHERE document_id = $1 \
         AND status IN ('CREATED','ASSIGNED','IN_PROGRESS')",
    )
    .bind(document)
    .fetch_one(&app.pool)
    .await
    .expect("the open task")
}

impl Walk {
    async fn start() -> Self {
        let app = TestApp::spawn_with(|config| config.integration_allow_loopback = true).await;
        let admin = app.administrator_token().await;
        let admin_id = user_id(&app, ADMIN_USERNAME).await;
        let approver_role = fixtures::create_role_with_permissions(
            &app.pool,
            SYSTEM_TENANT_ID,
            APPROVER_ROLE,
            &APPROVER_PERMISSIONS,
        )
        .await;
        let mock = mock_system().await;

        Self {
            app,
            admin,
            admin_id,
            approver_role,
            mock,
            serial: 0,
        }
    }

    fn next(&mut self) -> usize {
        self.serial += 1;
        self.serial
    }

    fn admin(&self) -> Caller {
        Caller {
            token: Some(self.admin.clone()),
            actor: Some(self.admin_id),
        }
    }

    /// A request the setup needs to succeed; its answer's `data`.
    async fn ok(&self, method: Method, uri: &str, token: &str, body: Option<Value>) -> Value {
        let response = self.app.send(method.clone(), uri, Some(token), body).await;
        assert!(
            response.status.is_success(),
            "the setup's {method} {uri} answered {} {}",
            response.status,
            response.body
        );
        response.data().clone()
    }

    async fn post(&self, uri: &str, body: Value) -> Value {
        self.ok(Method::POST, uri, &self.admin, Some(body)).await
    }

    async fn put(&self, uri: &str, body: Value) -> Value {
        self.ok(Method::PUT, uri, &self.admin, Some(body)).await
    }

    async fn permission(&self, code: &str) -> Uuid {
        sqlx::query_scalar("SELECT id FROM permissions WHERE permission_code = $1")
            .bind(code)
            .fetch_one(&self.app.pool)
            .await
            .expect("a seeded permission")
    }

    // -- People --------------------------------------------------------------

    /// A user, written by the fixture: its id and its username.
    async fn user(&mut self, roles: &[Uuid]) -> (Uuid, String) {
        let username = format!("walk.user{}", self.next());
        let id = fixtures::create_user(
            &self.app.pool,
            SYSTEM_TENANT_ID,
            &username,
            &format!("{username}@example.test"),
            PASSWORD,
            roles,
        )
        .await;
        (id, username)
    }

    /// A user, signed in.
    async fn signed_in(&mut self, roles: &[Uuid]) -> Caller {
        let (id, username) = self.user(roles).await;
        let token = self.app.sign_in(&username, PASSWORD).await;
        Caller {
            token: Some(token),
            actor: Some(id),
        }
    }

    async fn approver(&mut self) -> Caller {
        let role = self.approver_role;
        self.signed_in(&[role]).await
    }

    /// A user signed in through the route: its id, and its refresh token.
    async fn session(&mut self) -> (Uuid, String) {
        let (id, username) = self.user(&[]).await;
        let response = self
            .app
            .post(
                "/api/v1/auth/login",
                None,
                json!({ "username": username, "password": PASSWORD }),
            )
            .await;
        assert_eq!(response.status, StatusCode::OK, "{}", response.body);
        let refresh = response.data()["refreshToken"]
            .as_str()
            .expect("a refresh token")
            .to_owned();
        (id, refresh)
    }

    /// A reset token for `username`, read out of the mail as a person would.
    async fn reset_token(&self, username: &str) -> String {
        let delivered = self.app.sent_mail().len();
        let asked = self
            .app
            .post(
                "/api/v1/auth/forgot-password",
                None,
                json!({ "username": username }),
            )
            .await;
        assert_eq!(asked.status, StatusCode::ACCEPTED, "{}", asked.body);

        let mail = self.app.mail_delivered(delivered + 1).await;
        let address = format!("{username}@example.test");
        let last = mail
            .iter()
            .rev()
            .find(|mail| mail.to == address)
            .expect("the reset mail");

        let marker = "token=";
        let start = last.body.find(marker).expect("a link") + marker.len();
        last.body[start..]
            .chars()
            .take_while(char::is_ascii_alphanumeric)
            .collect()
    }

    async fn role(&mut self) -> Uuid {
        let code = format!("ROLE-WALK-{}", self.next());
        fixtures::create_role_with_permissions(&self.app.pool, SYSTEM_TENANT_ID, &code, &[]).await
    }

    async fn delegation(&mut self) -> Uuid {
        let (delegate, _) = self.user(&[]).await;
        let year = 2100 + self.next();
        id_of(
            &self
                .post(
                    "/api/v1/identity/delegations",
                    json!({
                        "delegateUserId": delegate,
                        "startsAt": format!("{year}-01-01T00:00:00Z"),
                        "endsAt": format!("{year}-02-01T00:00:00Z"),
                    }),
                )
                .await,
        )
    }

    // -- Documents -----------------------------------------------------------

    /// A form with the one field the workflow's variable reads.
    async fn form(&mut self, published: bool) -> Uuid {
        let key = format!("walk-form-{}", self.next());
        let form = id_of(
            &self
                .post(
                    "/api/v1/rad/forms",
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
                .await,
        );

        if published {
            self.post(&format!("/api/v1/rad/forms/{form}/publish"), json!({}))
                .await;
        }

        form
    }

    /// A type with no form, no rule and no workflow.
    async fn bare_type(&mut self) -> Uuid {
        let code = format!("WALK_BARE_{}", self.next());
        id_of(
            &self
                .post(
                    "/api/v1/document-types",
                    json!({ "typeCode": code, "name": code }),
                )
                .await,
        )
    }

    async fn numbering_rule(&self, type_id: Uuid, code: &str) {
        self.put(
            &format!("/api/v1/document-types/{type_id}/numbering-rule"),
            json!({
                "ruleTemplate": format!("{code}-{{year}}-{{sequence}}"),
                "sequenceScope": "YEAR",
                "gapPolicy": "GAPLESS",
            }),
        )
        .await;
    }

    /// A type with a published form and a numbering rule, and optionally a
    /// workflow.
    async fn document_type(&mut self, workflow: Option<Uuid>) -> Uuid {
        let code = format!("WALK_{}", self.next());
        let form = self.form(true).await;
        let mut body = json!({ "typeCode": code, "name": code, "formId": form });
        if let Some(workflow) = workflow {
            body["workflows"] = json!([{ "workflowDefinitionId": workflow }]);
        }

        let type_id = id_of(&self.post("/api/v1/document-types", body).await);
        self.numbering_rule(type_id, &code).await;
        type_id
    }

    async fn draft(&mut self, workflow: Option<Uuid>) -> Uuid {
        let type_id = self.document_type(workflow).await;
        id_of(
            &self
                .post(
                    "/api/v1/documents",
                    json!({
                        "documentTypeId": type_id,
                        "title": "Two standing desks",
                        "formData": { "amount": 45_000_000 },
                    }),
                )
                .await,
        )
    }

    async fn submit(&self, document: Uuid) {
        self.ok(
            Method::POST,
            &format!("/api/v1/documents/{document}/submission"),
            &self.admin,
            None,
        )
        .await;
    }

    async fn attachment(&self, document: Uuid) -> Uuid {
        let uploaded = self
            .app
            .post_multipart(
                &format!("/api/v1/documents/{document}/attachments"),
                Some(&self.admin),
                "quotation.pdf",
                "application/pdf",
                PDF,
                None,
            )
            .await;
        assert!(uploaded.status.is_success(), "{}", uploaded.body);
        id_of(uploaded.data())
    }

    async fn comment(&self, document: Uuid) -> Uuid {
        id_of(
            &self
                .post(
                    &format!("/api/v1/documents/{document}/comments"),
                    json!({ "body": "Please add the delivery date" }),
                )
                .await,
        )
    }

    async fn reference(&self, document: Uuid) -> Uuid {
        id_of(
            &self
                .post(
                    &format!("/api/v1/documents/{document}/references"),
                    json!({
                        "label": "Vendor portal",
                        "url": "https://vendor.example.test/quotes/2026-11",
                        "categoryId": QUOTATION,
                    }),
                )
                .await,
        )
    }

    // -- Workflow ------------------------------------------------------------

    async fn workflow(&mut self, published: bool) -> Uuid {
        let key = format!("wf_walk_{}", self.next());
        let id = id_of(
            &self
                .post(
                    "/api/v1/workflow/definitions",
                    json!({ "workflowKey": key, "name": "Standard approval",
                            "definition": approval(&key) }),
                )
                .await,
        );

        if published {
            self.post(
                &format!("/api/v1/workflow/definitions/{id}/publication"),
                json!({}),
            )
            .await;
        }

        id
    }

    /// A document awaiting its approval: the document, and its open task.
    async fn awaiting_approval(&mut self) -> (Uuid, Uuid) {
        let workflow = self.workflow(true).await;
        let document = self.draft(Some(workflow)).await;
        self.submit(document).await;
        (document, open_task(&self.app, document).await)
    }

    // -- Integration ---------------------------------------------------------

    async fn system(&mut self) -> Uuid {
        let code = format!("SYS_WALK_{}", self.next());
        id_of(
            &self
                .post(
                    "/api/v1/integration/external-systems",
                    json!({
                        "systemCode": code,
                        "systemName": "Walk system",
                        "baseUrl": self.mock,
                        "timeoutSeconds": 2,
                    }),
                )
                .await,
        )
    }

    async fn endpoint(&mut self, system: Uuid) -> Uuid {
        let code = format!("EP_WALK_{}", self.next());
        id_of(
            &self
                .post(
                    &format!("/api/v1/integration/external-systems/{system}/endpoints"),
                    json!({ "endpointCode": code, "name": "Probe", "method": "GET", "path": "/probe" }),
                )
                .await,
        )
    }

    async fn credential(&mut self, system: Uuid) -> Uuid {
        let n = self.next();
        id_of(
            &self
                .post(
                    &format!("/api/v1/integration/external-systems/{system}/credentials"),
                    json!({
                        "credentialType": "API_KEY",
                        "secretReference": format!("vault://kelir/walk/{n}"),
                    }),
                )
                .await,
        )
    }

    // -- Master data, organization, RAD --------------------------------------

    async fn party(&mut self) -> Uuid {
        let code = format!("PARTY-WALK-{}", self.next());
        id_of(
            &self
                .post(
                    "/api/v1/master-data/parties",
                    json!({
                        "partyId": code,
                        "partyTypeId": "PARTY_GROUP",
                        "partyGroup": { "groupName": "Acme Supplies" },
                    }),
                )
                .await,
        )
    }

    async fn facility(&mut self) -> Uuid {
        let code = format!("FAC-WALK-{}", self.next());
        id_of(
            &self
                .post(
                    "/api/v1/master-data/facilities",
                    json!({ "facilityId": code, "name": "North warehouse",
                            "facilityTypeId": "BUILDING" }),
                )
                .await,
        )
    }

    fn supplier(&mut self) -> Value {
        json!({
            "fromDate": "2026-01-01T00:00:00Z",
            "profile": {
                "supplier": {
                    "supplierNumber": format!("SUP-WALK-{}", self.next()),
                    "supplierCategory": "IT",
                    "paymentTermDays": 30,
                    "defaultCurrencyUom": "IDR",
                    "bankName": "Bank Mandiri",
                    "bankAccount": "1234567890",
                    "bankAccountName": "Acme Supplies",
                    "approvalStatus": "APPROVED",
                }
            },
        })
    }

    async fn department(&mut self) -> Uuid {
        let code = format!("DEPT-WALK-{}", self.next());
        id_of(
            &self
                .post(
                    "/api/v1/organization/departments",
                    json!({ "departmentId": code, "name": "Procurement" }),
                )
                .await,
        )
    }

    async fn tenant(&mut self) -> Uuid {
        let code = format!("TNT-WALK-{}", self.next());
        fixtures::create_tenant(&self.app.pool, &code, "A walked tenant").await
    }

    fn list_body(&mut self) -> Value {
        json!({
            "listKey": format!("walk-list-{}", self.next()),
            "title": "Purchase requisitions",
            "columns": [
                { "columnKey": "document_number", "label": "Number", "dataType": "STRING" }
            ],
        })
    }

    fn menu_body(&mut self) -> Value {
        json!({
            "menuKey": format!("walk-menu-{}", self.next()),
            "label": "Documents",
            "routePath": "/documents",
        })
    }
}

/// **Each cell**: its subject, created through the API, and the request and
/// row that are its claim. `None` for a name with no cell, which the walk
/// reports.
async fn setup(walk: &mut Walk, name: &str) -> Option<Cell> {
    use Subject::{Answered, Known};

    let made = match name {
        // -- Authentication --------------------------------------------------
        "POST /api/v1/auth/login" => {
            let (user, username) = walk.user(&[]).await;
            cell(
                Caller {
                    token: None,
                    actor: Some(user),
                },
                "/api/v1/auth/login".into(),
                Body::Json(json!({ "username": username, "password": PASSWORD })),
                "Security.SignedIn",
                "USER",
                Known(user),
            )
        }
        "POST /api/v1/auth/logout" => {
            let (user, refresh) = walk.session().await;
            cell(
                Caller {
                    token: None,
                    actor: Some(user),
                },
                "/api/v1/auth/logout".into(),
                Body::Json(json!({ "refreshToken": refresh })),
                "Security.SignedOut",
                "USER",
                Known(user),
            )
        }
        "POST /api/v1/auth/change-password" => {
            let caller = walk.signed_in(&[]).await;
            let user = caller.actor.expect("a user");
            cell(
                caller,
                "/api/v1/auth/change-password".into(),
                Body::Json(json!({ "currentPassword": PASSWORD, "newPassword": NEW_PASSWORD })),
                "User.PasswordChanged",
                "USER",
                Known(user),
            )
        }
        "POST /api/v1/auth/forgot-password" => {
            let (user, username) = walk.user(&[]).await;
            // No actor: whoever asks for a reset has proved nothing.
            cell(
                Caller {
                    token: None,
                    actor: None,
                },
                "/api/v1/auth/forgot-password".into(),
                Body::Json(json!({ "username": username })),
                "User.PasswordResetRequested",
                "USER",
                Known(user),
            )
        }
        "POST /api/v1/auth/reset-password" => {
            let (user, username) = walk.user(&[]).await;
            let token = walk.reset_token(&username).await;
            // No actor: holding the token is not proving who you are.
            cell(
                Caller {
                    token: None,
                    actor: None,
                },
                "/api/v1/auth/reset-password".into(),
                Body::Json(json!({ "token": token, "newPassword": NEW_PASSWORD })),
                "User.PasswordReset",
                "USER",
                Known(user),
            )
        }

        // -- Document types --------------------------------------------------
        "POST /api/v1/document-types" => {
            let code = format!("WALK_NEW_{}", walk.next());
            cell(
                walk.admin(),
                "/api/v1/document-types".into(),
                Body::Json(json!({ "typeCode": code, "name": code })),
                "DocumentType.Created",
                "DOCUMENT_TYPE",
                Answered,
            )
        }
        "PUT /api/v1/document-types/{id}" => {
            let type_id = walk.bare_type().await;
            cell(
                walk.admin(),
                format!("/api/v1/document-types/{type_id}"),
                Body::Json(json!({ "name": "Renamed by the walk" })),
                "DocumentType.Updated",
                "DOCUMENT_TYPE",
                Known(type_id),
            )
        }
        "DELETE /api/v1/document-types/{id}" => {
            let type_id = walk.bare_type().await;
            cell(
                walk.admin(),
                format!("/api/v1/document-types/{type_id}"),
                Body::Empty,
                "DocumentType.Deleted",
                "DOCUMENT_TYPE",
                Known(type_id),
            )
        }
        "PUT /api/v1/document-types/{id}/numbering-rule" => {
            let type_id = walk.bare_type().await;
            cell(
                walk.admin(),
                format!("/api/v1/document-types/{type_id}/numbering-rule"),
                Body::Json(json!({
                    "ruleTemplate": format!("WALK{}-{{year}}-{{sequence}}", walk.next()),
                    "sequenceScope": "YEAR",
                    "gapPolicy": "GAPLESS",
                })),
                "DocumentTypeNumberingRule.Set",
                "DOCUMENT_TYPE_NUMBERING_RULE",
                Known(type_id),
            )
        }
        "DELETE /api/v1/document-types/{id}/numbering-rule" => {
            let type_id = walk.bare_type().await;
            let code = format!("WALKRULE{}", walk.next());
            walk.numbering_rule(type_id, &code).await;
            cell(
                walk.admin(),
                format!("/api/v1/document-types/{type_id}/numbering-rule"),
                Body::Empty,
                "DocumentTypeNumberingRule.Cleared",
                "DOCUMENT_TYPE_NUMBERING_RULE",
                Known(type_id),
            )
        }

        // -- Documents -------------------------------------------------------
        "POST /api/v1/documents" => {
            let type_id = walk.document_type(None).await;
            cell(
                walk.admin(),
                "/api/v1/documents".into(),
                Body::Json(json!({
                    "documentTypeId": type_id,
                    "title": "Two standing desks",
                    "formData": { "amount": 45_000_000 },
                })),
                "Document.Created",
                "DOCUMENT",
                Answered,
            )
        }
        "PUT /api/v1/documents/{id}" => {
            let document = walk.draft(None).await;
            cell(
                walk.admin(),
                format!("/api/v1/documents/{document}"),
                Body::Json(json!({ "title": "Three standing desks" })),
                "Document.Updated",
                "DOCUMENT",
                Known(document),
            )
        }
        "DELETE /api/v1/documents/{id}" => {
            let document = walk.draft(None).await;
            cell(
                walk.admin(),
                format!("/api/v1/documents/{document}"),
                Body::Empty,
                "Document.Deleted",
                "DOCUMENT",
                Known(document),
            )
        }
        "POST /api/v1/documents/{id}/submission" => {
            let document = walk.draft(None).await;
            cell(
                walk.admin(),
                format!("/api/v1/documents/{document}/submission"),
                Body::Empty,
                "Document.Submitted",
                "DOCUMENT",
                Known(document),
            )
        }
        "PUT /api/v1/documents/{id}/status" => {
            let document = walk.draft(None).await;
            walk.submit(document).await;
            cell(
                walk.admin(),
                format!("/api/v1/documents/{document}/status"),
                Body::Json(json!({ "status": "IN_REVIEW" })),
                "Document.StatusChanged",
                "DOCUMENT",
                Known(document),
            )
        }
        "POST /api/v1/documents/{id}/attachments" => {
            let document = walk.draft(None).await;
            cell(
                walk.admin(),
                format!("/api/v1/documents/{document}/attachments"),
                Body::Pdf,
                "Attachment.Added",
                "ATTACHMENT",
                Answered,
            )
        }
        "DELETE /api/v1/documents/{id}/attachments/{attachment_id}" => {
            let document = walk.draft(None).await;
            let attachment = walk.attachment(document).await;
            cell(
                walk.admin(),
                format!("/api/v1/documents/{document}/attachments/{attachment}"),
                Body::Empty,
                "Attachment.Deleted",
                "ATTACHMENT",
                Known(attachment),
            )
        }
        "POST /api/v1/documents/{id}/comments" => {
            let document = walk.draft(None).await;
            cell(
                walk.admin(),
                format!("/api/v1/documents/{document}/comments"),
                Body::Json(json!({ "body": "Please add the delivery date" })),
                "Comment.Added",
                "COMMENT",
                Answered,
            )
        }
        "PUT /api/v1/documents/{id}/comments/{comment_id}" => {
            let document = walk.draft(None).await;
            let comment = walk.comment(document).await;
            cell(
                walk.admin(),
                format!("/api/v1/documents/{document}/comments/{comment}"),
                Body::Json(json!({ "body": "Please add the delivery date and the site" })),
                "Comment.Edited",
                "COMMENT",
                Known(comment),
            )
        }
        "DELETE /api/v1/documents/{id}/comments/{comment_id}" => {
            let document = walk.draft(None).await;
            let comment = walk.comment(document).await;
            cell(
                walk.admin(),
                format!("/api/v1/documents/{document}/comments/{comment}"),
                Body::Empty,
                "Comment.Deleted",
                "COMMENT",
                Known(comment),
            )
        }
        "POST /api/v1/documents/{id}/references" => {
            let document = walk.draft(None).await;
            cell(
                walk.admin(),
                format!("/api/v1/documents/{document}/references"),
                Body::Json(json!({
                    "label": "Vendor portal",
                    "url": "https://vendor.example.test/quotes/2026-11",
                    "categoryId": QUOTATION,
                })),
                "Reference.Added",
                "EXTERNAL_REFERENCE",
                Answered,
            )
        }
        "DELETE /api/v1/documents/{id}/references/{reference_id}" => {
            let document = walk.draft(None).await;
            let reference = walk.reference(document).await;
            cell(
                walk.admin(),
                format!("/api/v1/documents/{document}/references/{reference}"),
                Body::Empty,
                "Reference.Deleted",
                "EXTERNAL_REFERENCE",
                Known(reference),
            )
        }

        // -- Identity --------------------------------------------------------
        "POST /api/v1/identity/users" => {
            let role = walk.role().await;
            let username = format!("walk.made{}", walk.next());
            cell(
                walk.admin(),
                "/api/v1/identity/users".into(),
                Body::Json(json!({
                    "username": username,
                    "email": format!("{username}@example.test"),
                    "password": PASSWORD,
                    "displayName": "Made by the walk",
                    "roleIds": [role],
                })),
                "User.Created",
                "USER",
                Answered,
            )
        }
        "PUT /api/v1/identity/users/{id}" => {
            let (user, _) = walk.user(&[]).await;
            cell(
                walk.admin(),
                format!("/api/v1/identity/users/{user}"),
                Body::Json(json!({ "displayName": "Renamed by the walk" })),
                "User.Updated",
                "USER",
                Known(user),
            )
        }
        "DELETE /api/v1/identity/users/{id}" => {
            let (user, _) = walk.user(&[]).await;
            cell(
                walk.admin(),
                format!("/api/v1/identity/users/{user}"),
                Body::Empty,
                // **Deliberately wrong, for the red-first run** (#960): this
                // route writes `User.Deactivated`, and the next commit says so.
                "User.Deleted",
                "USER",
                Known(user),
            )
        }
        "POST /api/v1/identity/users/{id}/password" => {
            let (user, _) = walk.user(&[]).await;
            cell(
                walk.admin(),
                format!("/api/v1/identity/users/{user}/password"),
                Body::Json(json!({ "password": NEW_PASSWORD })),
                "User.PasswordChanged",
                "USER",
                Known(user),
            )
        }
        "POST /api/v1/identity/roles" => {
            let permission = walk.permission("identity:user:read").await;
            let code = format!("ROLE-WALK-NEW-{}", walk.next());
            cell(
                walk.admin(),
                "/api/v1/identity/roles".into(),
                Body::Json(json!({
                    "roleCode": code,
                    "name": "Made by the walk",
                    "permissionIds": [permission],
                })),
                "Role.Created",
                "ROLE",
                Answered,
            )
        }
        "PUT /api/v1/identity/roles/{id}" => {
            // What #1000 gets wrong is the row's content, which
            // `a_role_edit_audits_permission_ids_it_was_not_sent_issue_1000`
            // pins. This cell sends the ids, and asserts only the row.
            let role = walk.role().await;
            let permission = walk.permission("identity:user:read").await;
            cell(
                walk.admin(),
                format!("/api/v1/identity/roles/{role}"),
                Body::Json(json!({ "name": "Renamed by the walk", "permissionIds": [permission] })),
                "Role.Updated",
                "ROLE",
                Known(role),
            )
        }
        "DELETE /api/v1/identity/roles/{id}" => {
            let role = walk.role().await;
            cell(
                walk.admin(),
                format!("/api/v1/identity/roles/{role}"),
                Body::Empty,
                "Role.Deleted",
                "ROLE",
                Known(role),
            )
        }
        "POST /api/v1/identity/delegations" => {
            let (delegate, _) = walk.user(&[]).await;
            let year = 2100 + walk.next();
            cell(
                walk.admin(),
                "/api/v1/identity/delegations".into(),
                Body::Json(json!({
                    "delegateUserId": delegate,
                    "startsAt": format!("{year}-01-01T00:00:00Z"),
                    "endsAt": format!("{year}-02-01T00:00:00Z"),
                })),
                "Identity.DelegationOpened",
                "DELEGATION",
                Answered,
            )
        }
        "DELETE /api/v1/identity/delegations/{id}" => {
            let delegation = walk.delegation().await;
            cell(
                walk.admin(),
                format!("/api/v1/identity/delegations/{delegation}"),
                Body::Empty,
                "Identity.DelegationEnded",
                "DELEGATION",
                Known(delegation),
            )
        }

        // -- Integration -----------------------------------------------------
        "POST /api/v1/integration/external-systems" => {
            let code = format!("SYS_WALK_NEW_{}", walk.next());
            cell(
                walk.admin(),
                "/api/v1/integration/external-systems".into(),
                Body::Json(json!({ "systemCode": code, "systemName": "Made by the walk" })),
                "ExternalSystem.Registered",
                "EXTERNAL_SYSTEM",
                Answered,
            )
        }
        "PUT /api/v1/integration/external-systems/{id}" => {
            let system = walk.system().await;
            cell(
                walk.admin(),
                format!("/api/v1/integration/external-systems/{system}"),
                Body::Json(json!({ "systemName": "Renamed by the walk" })),
                "ExternalSystem.Updated",
                "EXTERNAL_SYSTEM",
                Known(system),
            )
        }
        "POST /api/v1/integration/external-systems/{id}/deactivate" => {
            let system = walk.system().await;
            cell(
                walk.admin(),
                format!("/api/v1/integration/external-systems/{system}/deactivate"),
                Body::Empty,
                "ExternalSystem.Deactivated",
                "EXTERNAL_SYSTEM",
                Known(system),
            )
        }
        "POST /api/v1/integration/external-systems/{id}/activate" => {
            let system = walk.system().await;
            walk.ok(
                Method::POST,
                &format!("/api/v1/integration/external-systems/{system}/deactivate"),
                &walk.admin,
                None,
            )
            .await;
            cell(
                walk.admin(),
                format!("/api/v1/integration/external-systems/{system}/activate"),
                Body::Empty,
                "ExternalSystem.Activated",
                "EXTERNAL_SYSTEM",
                Known(system),
            )
        }
        "POST /api/v1/integration/external-systems/{id}/endpoints" => {
            let system = walk.system().await;
            let code = format!("EP_WALK_NEW_{}", walk.next());
            cell(
                walk.admin(),
                format!("/api/v1/integration/external-systems/{system}/endpoints"),
                Body::Json(json!({
                    "endpointCode": code, "name": "Made by the walk",
                    "method": "POST", "path": "/made"
                })),
                "IntegrationEndpoint.Created",
                "INTEGRATION_ENDPOINT",
                Answered,
            )
        }
        "PUT /api/v1/integration/external-systems/{id}/endpoints/{endpointId}" => {
            let system = walk.system().await;
            let endpoint = walk.endpoint(system).await;
            cell(
                walk.admin(),
                format!("/api/v1/integration/external-systems/{system}/endpoints/{endpoint}"),
                Body::Json(json!({ "name": "Renamed by the walk" })),
                "IntegrationEndpoint.Updated",
                "INTEGRATION_ENDPOINT",
                Known(endpoint),
            )
        }
        "POST /api/v1/integration/external-systems/{id}/credentials" => {
            let system = walk.system().await;
            let n = walk.next();
            cell(
                walk.admin(),
                format!("/api/v1/integration/external-systems/{system}/credentials"),
                Body::Json(json!({
                    "credentialType": "API_KEY",
                    "secretReference": format!("vault://kelir/walk/made/{n}"),
                })),
                "IntegrationCredential.Created",
                "INTEGRATION_CREDENTIAL",
                Answered,
            )
        }
        "PUT /api/v1/integration/external-systems/{id}/credentials/{credentialId}" => {
            let system = walk.system().await;
            let credential = walk.credential(system).await;
            cell(
                walk.admin(),
                format!("/api/v1/integration/external-systems/{system}/credentials/{credential}"),
                Body::Json(json!({ "isActive": false })),
                "IntegrationCredential.Updated",
                "INTEGRATION_CREDENTIAL",
                Known(credential),
            )
        }
        "DELETE /api/v1/integration/external-systems/{id}/credentials/{credentialId}" => {
            let system = walk.system().await;
            let credential = walk.credential(system).await;
            cell(
                walk.admin(),
                format!("/api/v1/integration/external-systems/{system}/credentials/{credential}"),
                Body::Empty,
                "IntegrationCredential.Deleted",
                "INTEGRATION_CREDENTIAL",
                Known(credential),
            )
        }

        // -- Master data -----------------------------------------------------
        "POST /api/v1/master-data/facilities" => {
            let code = format!("FAC-WALK-NEW-{}", walk.next());
            cell(
                walk.admin(),
                "/api/v1/master-data/facilities".into(),
                Body::Json(json!({ "facilityId": code, "name": "Central warehouse",
                                   "facilityTypeId": "WAREHOUSE" })),
                "Facility.Created",
                "FACILITY",
                Answered,
            )
        }
        "PUT /api/v1/master-data/facilities/{id}" => {
            let facility = walk.facility().await;
            cell(
                walk.admin(),
                format!("/api/v1/master-data/facilities/{facility}"),
                Body::Json(json!({ "name": "Renamed by the walk" })),
                "Facility.Updated",
                "FACILITY",
                Known(facility),
            )
        }
        "DELETE /api/v1/master-data/facilities/{id}" => {
            let facility = walk.facility().await;
            cell(
                walk.admin(),
                format!("/api/v1/master-data/facilities/{facility}"),
                Body::Empty,
                "Facility.Deleted",
                "FACILITY",
                Known(facility),
            )
        }
        "POST /api/v1/master-data/facilities/{id}/transition" => {
            let facility = walk.facility().await;
            cell(
                walk.admin(),
                format!("/api/v1/master-data/facilities/{facility}/transition"),
                Body::Json(json!({ "recordStatusId": "ACTIVE" })),
                "Facility.RecordStatusChanged",
                "FACILITY",
                Known(facility),
            )
        }
        "POST /api/v1/master-data/parties" => {
            let code = format!("PARTY-WALK-NEW-{}", walk.next());
            cell(
                walk.admin(),
                "/api/v1/master-data/parties".into(),
                Body::Json(json!({
                    "partyId": code,
                    "partyTypeId": "PARTY_GROUP",
                    "partyGroup": { "groupName": "Acme Supplies" },
                })),
                "Party.Created",
                "PARTY",
                Answered,
            )
        }
        "PUT /api/v1/master-data/parties/{id}" => {
            let party = walk.party().await;
            cell(
                walk.admin(),
                format!("/api/v1/master-data/parties/{party}"),
                Body::Json(json!({ "description": "Primary supplier" })),
                "Party.Updated",
                "PARTY",
                Known(party),
            )
        }
        "DELETE /api/v1/master-data/parties/{id}" => {
            let party = walk.party().await;
            cell(
                walk.admin(),
                format!("/api/v1/master-data/parties/{party}"),
                Body::Empty,
                "Party.Deleted",
                "PARTY",
                Known(party),
            )
        }
        "PUT /api/v1/master-data/parties/{id}/roles/{roleTypeId}" => {
            let party = walk.party().await;
            let profile = walk.supplier();
            cell(
                walk.admin(),
                format!("/api/v1/master-data/parties/{party}/roles/SUPPLIER"),
                Body::Json(profile),
                "Supplier.Created",
                "PARTY",
                Known(party),
            )
        }
        "DELETE /api/v1/master-data/parties/{id}/roles/{roleTypeId}" => {
            let party = walk.party().await;
            let profile = walk.supplier();
            walk.put(
                &format!("/api/v1/master-data/parties/{party}/roles/SUPPLIER"),
                profile,
            )
            .await;
            cell(
                walk.admin(),
                format!("/api/v1/master-data/parties/{party}/roles/SUPPLIER"),
                Body::Empty,
                "Supplier.Removed",
                "PARTY",
                Known(party),
            )
        }
        "POST /api/v1/master-data/parties/{id}/transition" => {
            let party = walk.party().await;
            cell(
                walk.admin(),
                format!("/api/v1/master-data/parties/{party}/transition"),
                Body::Json(json!({ "recordStatusId": "ACTIVE" })),
                "Party.RecordStatusChanged",
                "PARTY",
                Known(party),
            )
        }

        // -- Organization ----------------------------------------------------
        "POST /api/v1/organization/departments" => {
            let code = format!("DEPT-WALK-NEW-{}", walk.next());
            cell(
                walk.admin(),
                "/api/v1/organization/departments".into(),
                Body::Json(json!({ "departmentId": code, "name": "Finance" })),
                "Department.Created",
                "DEPARTMENT",
                Answered,
            )
        }
        "PUT /api/v1/organization/departments/{id}" => {
            let department = walk.department().await;
            cell(
                walk.admin(),
                format!("/api/v1/organization/departments/{department}"),
                Body::Json(json!({ "name": "Renamed by the walk" })),
                "Department.Updated",
                "DEPARTMENT",
                Known(department),
            )
        }
        "DELETE /api/v1/organization/departments/{id}" => {
            let department = walk.department().await;
            cell(
                walk.admin(),
                format!("/api/v1/organization/departments/{department}"),
                Body::Empty,
                "Department.Deleted",
                "DEPARTMENT",
                Known(department),
            )
        }
        "POST /api/v1/organization/tenants" => {
            // Tenant provisioning, one of the mutations that is not a route of
            // its own: the first administrator is provisioned in this request,
            // and the row is written in the administering tenant.
            let n = walk.next();
            let username = format!("walk.tenant.admin{n}");
            cell(
                walk.admin(),
                "/api/v1/organization/tenants".into(),
                Body::Json(json!({
                    "tenantCode": format!("TNT-WALK-NEW-{n}"),
                    "name": "A tenant the walk made",
                    "administrator": {
                        "username": username,
                        "email": format!("{username}@example.test"),
                        "displayName": "Tenant Administrator",
                        "password": PASSWORD,
                    },
                })),
                "Tenant.Created",
                "TENANT",
                Answered,
            )
        }
        "PUT /api/v1/organization/tenants/{id}" => {
            // What #1001 gets wrong is the row's content, which
            // `a_tenant_status_edit_audits_a_name_it_was_not_sent_issue_1001`
            // pins. This cell asserts only the row.
            let tenant = walk.tenant().await;
            cell(
                walk.admin(),
                format!("/api/v1/organization/tenants/{tenant}"),
                Body::Json(json!({ "name": "Renamed by the walk" })),
                "Tenant.Updated",
                "TENANT",
                Known(tenant),
            )
        }
        "DELETE /api/v1/organization/tenants/{id}" => {
            let tenant = walk.tenant().await;
            cell(
                walk.admin(),
                format!("/api/v1/organization/tenants/{tenant}"),
                Body::Empty,
                "Tenant.Deleted",
                "TENANT",
                Known(tenant),
            )
        }

        // -- RAD -------------------------------------------------------------
        "POST /api/v1/rad/forms" => {
            let key = format!("walk-form-new-{}", walk.next());
            cell(
                walk.admin(),
                "/api/v1/rad/forms".into(),
                Body::Json(json!({
                    "formKey": key,
                    "title": "Purchase requisition",
                    "definition": {
                        "formId": key,
                        "version": "2.0.1",
                        "title": "Purchase requisition",
                        "components": [{
                            "id": "quantity", "role": "data", "type": "number",
                            "key": "quantity", "label": "Quantity",
                            "validation": { "type": "number" }
                        }]
                    },
                })),
                "RadForm.Created",
                "RAD_FORM",
                Answered,
            )
        }
        "PUT /api/v1/rad/forms/{id}" => {
            let form = walk.form(false).await;
            cell(
                walk.admin(),
                format!("/api/v1/rad/forms/{form}"),
                Body::Json(json!({ "title": "Renamed by the walk" })),
                "RadForm.Updated",
                "RAD_FORM",
                Known(form),
            )
        }
        "DELETE /api/v1/rad/forms/{id}" => {
            let form = walk.form(false).await;
            cell(
                walk.admin(),
                format!("/api/v1/rad/forms/{form}"),
                Body::Empty,
                "RadForm.Deleted",
                "RAD_FORM",
                Known(form),
            )
        }
        "POST /api/v1/rad/forms/{id}/publish" => {
            let form = walk.form(false).await;
            cell(
                walk.admin(),
                format!("/api/v1/rad/forms/{form}/publish"),
                Body::Empty,
                "RadForm.Published",
                "RAD_FORM",
                Known(form),
            )
        }
        "POST /api/v1/rad/forms/{id}/revisions" => {
            let form = walk.form(true).await;
            cell(
                walk.admin(),
                format!("/api/v1/rad/forms/{form}/revisions"),
                Body::Json(json!({})),
                "RadForm.RevisionCreated",
                "RAD_FORM",
                Answered,
            )
        }
        "POST /api/v1/rad/forms/{id}/submissions" => {
            let form = walk.form(true).await;
            cell(
                walk.admin(),
                format!("/api/v1/rad/forms/{form}/submissions"),
                Body::Json(json!({ "payload": { "amount": 5 } })),
                "RadForm.Submitted",
                "RAD_FORM_SUBMISSION",
                Answered,
            )
        }
        "POST /api/v1/rad/lists" => {
            let body = walk.list_body();
            cell(
                walk.admin(),
                "/api/v1/rad/lists".into(),
                Body::Json(body),
                "RadList.Created",
                "RAD_LIST",
                Answered,
            )
        }
        "PUT /api/v1/rad/lists/{id}" => {
            let body = walk.list_body();
            let list = id_of(&walk.post("/api/v1/rad/lists", body).await);
            cell(
                walk.admin(),
                format!("/api/v1/rad/lists/{list}"),
                Body::Json(json!({ "title": "Renamed by the walk" })),
                "RadList.Updated",
                "RAD_LIST",
                Known(list),
            )
        }
        "DELETE /api/v1/rad/lists/{id}" => {
            let body = walk.list_body();
            let list = id_of(&walk.post("/api/v1/rad/lists", body).await);
            cell(
                walk.admin(),
                format!("/api/v1/rad/lists/{list}"),
                Body::Empty,
                "RadList.Deleted",
                "RAD_LIST",
                Known(list),
            )
        }
        "POST /api/v1/rad/menus" => {
            let body = walk.menu_body();
            cell(
                walk.admin(),
                "/api/v1/rad/menus".into(),
                Body::Json(body),
                "RadMenu.Created",
                "RAD_MENU",
                Answered,
            )
        }
        "PUT /api/v1/rad/menus/{id}" => {
            let body = walk.menu_body();
            let menu = id_of(&walk.post("/api/v1/rad/menus", body).await);
            cell(
                walk.admin(),
                format!("/api/v1/rad/menus/{menu}"),
                Body::Json(json!({ "label": "Renamed by the walk" })),
                "RadMenu.Updated",
                "RAD_MENU",
                Known(menu),
            )
        }
        "DELETE /api/v1/rad/menus/{id}" => {
            let body = walk.menu_body();
            let menu = id_of(&walk.post("/api/v1/rad/menus", body).await);
            cell(
                walk.admin(),
                format!("/api/v1/rad/menus/{menu}"),
                Body::Empty,
                "RadMenu.Deleted",
                "RAD_MENU",
                Known(menu),
            )
        }

        // -- Workflow --------------------------------------------------------
        "POST /api/v1/workflow/definitions" => {
            let key = format!("wf_walk_new_{}", walk.next());
            cell(
                walk.admin(),
                "/api/v1/workflow/definitions".into(),
                Body::Json(json!({ "workflowKey": key, "name": "Standard approval",
                                   "definition": approval(&key) })),
                "Workflow.Created",
                "WORKFLOW_DEFINITION",
                Answered,
            )
        }
        "PUT /api/v1/workflow/definitions/{id}" => {
            let definition = walk.workflow(false).await;
            cell(
                walk.admin(),
                format!("/api/v1/workflow/definitions/{definition}"),
                Body::Json(json!({ "name": "Renamed by the walk" })),
                "Workflow.Updated",
                "WORKFLOW_DEFINITION",
                Known(definition),
            )
        }
        "DELETE /api/v1/workflow/definitions/{id}" => {
            let definition = walk.workflow(false).await;
            cell(
                walk.admin(),
                format!("/api/v1/workflow/definitions/{definition}"),
                Body::Empty,
                "Workflow.Deleted",
                "WORKFLOW_DEFINITION",
                Known(definition),
            )
        }
        "POST /api/v1/workflow/definitions/{id}/publication" => {
            let definition = walk.workflow(false).await;
            cell(
                walk.admin(),
                format!("/api/v1/workflow/definitions/{definition}/publication"),
                Body::Json(json!({})),
                "Workflow.Published",
                "WORKFLOW_DEFINITION",
                Known(definition),
            )
        }
        "POST /api/v1/workflow/definitions/{id}/deprecation" => {
            let definition = walk.workflow(true).await;
            cell(
                walk.admin(),
                format!("/api/v1/workflow/definitions/{definition}/deprecation"),
                Body::Json(json!({})),
                "Workflow.Deprecated",
                "WORKFLOW_DEFINITION",
                Known(definition),
            )
        }
        "POST /api/v1/workflow/definitions/{id}/revisions" => {
            let definition = walk.workflow(true).await;
            cell(
                walk.admin(),
                format!("/api/v1/workflow/definitions/{definition}/revisions"),
                Body::Json(json!({})),
                "Workflow.RevisionCreated",
                "WORKFLOW_DEFINITION",
                Answered,
            )
        }
        "POST /api/v1/workflow/tasks/{id}/claim" => {
            let approver = walk.approver().await;
            let (_, task) = walk.awaiting_approval().await;
            cell(
                approver,
                format!("/api/v1/workflow/tasks/{task}/claim"),
                Body::Json(json!({})),
                "Workflow.TaskClaimed",
                "WORKFLOW_TASK",
                Known(task),
            )
        }
        "POST /api/v1/workflow/tasks/{id}/decision" => {
            let approver = walk.approver().await;
            let (_, task) = walk.awaiting_approval().await;
            cell(
                approver,
                format!("/api/v1/workflow/tasks/{task}/decision"),
                Body::Json(json!({ "action": "APPROVE" })),
                "Workflow.TaskCompleted",
                "WORKFLOW_TASK",
                Known(task),
            )
        }
        "POST /api/v1/workflow/tasks/{id}/delegation" => {
            let holder = walk.approver().await;
            let delegate = walk.approver().await;
            let (_, task) = walk.awaiting_approval().await;
            walk.ok(
                Method::POST,
                &format!("/api/v1/workflow/tasks/{task}/claim"),
                holder.token.as_deref().expect("a token"),
                Some(json!({})),
            )
            .await;
            cell(
                holder,
                format!("/api/v1/workflow/tasks/{task}/delegation"),
                Body::Json(json!({
                    "delegateUserId": delegate.actor,
                    "comment": "Away from tomorrow",
                })),
                "Workflow.TaskDelegated",
                "WORKFLOW_TASK",
                Known(task),
            )
        }
        "POST /api/v1/workflow/tasks/{id}/reassign" => {
            let target = walk.approver().await;
            let (_, task) = walk.awaiting_approval().await;
            cell(
                walk.admin(),
                format!("/api/v1/workflow/tasks/{task}/reassign"),
                Body::Json(json!({ "userId": target.actor })),
                "Workflow.TaskReassigned",
                "WORKFLOW_TASK",
                Known(task),
            )
        }

        _ => return None,
    };

    Some(made)
}

/// Each exempt operation's request: who sends it, where, and with what.
async fn exempt_setup(walk: &mut Walk, name: &str) -> Option<(Caller, String, Body)> {
    let request = match name {
        "POST /api/v1/auth/refresh" => {
            let (_, refresh) = walk.session().await;
            (
                Caller {
                    token: None,
                    actor: None,
                },
                "/api/v1/auth/refresh".to_owned(),
                Body::Json(json!({ "refreshToken": refresh })),
            )
        }
        "POST /api/v1/notifications/read" => {
            let approver = walk.approver().await;
            walk.awaiting_approval().await;
            (
                approver,
                "/api/v1/notifications/read".to_owned(),
                Body::Json(json!({})),
            )
        }
        "POST /api/v1/notifications/{id}/read" => {
            let approver = walk.approver().await;
            walk.awaiting_approval().await;
            let notification: Uuid = sqlx::query_scalar(
                "SELECT id FROM notifications WHERE recipient_user_id = $1 \
                 ORDER BY created_at DESC, id DESC LIMIT 1",
            )
            .bind(approver.actor)
            .fetch_one(&walk.app.pool)
            .await
            .expect("the approver was told of the task");
            (
                approver,
                format!("/api/v1/notifications/{notification}/read"),
                Body::Json(json!({})),
            )
        }
        "POST /api/v1/integration/external-systems/{id}/endpoints/{endpointId}/test-call" => {
            let system = walk.system().await;
            let endpoint = walk.endpoint(system).await;
            (
                walk.admin(),
                format!(
                    "/api/v1/integration/external-systems/{system}/endpoints/{endpoint}/test-call"
                ),
                Body::Json(json!({})),
            )
        }
        _ => return None,
    };

    Some(request)
}

// ---------------------------------------------------------------------------
// The known breaches, pinned at today's behaviour (A to E)
//
// Each passes today and asserts the exact wrong value. **The row that fixes
// the finding inverts its test and renames it**, so a fix that lands without
// touching the test turns it red.
// ---------------------------------------------------------------------------

/// The one row of `event_type` among `rows`.
fn the_one<'a>(rows: &'a [StoredRow], event_type: &str) -> &'a StoredRow {
    let matching: Vec<&StoredRow> = rows
        .iter()
        .filter(|row| row.event_type == event_type)
        .collect();
    assert_eq!(
        matching.len(),
        1,
        "one {event_type} row, and the trail holds {}",
        listed(rows)
    );
    matching[0]
}

/// **A, [#1000].** A role edit that sends only a name audits
/// `PERMISSION_CHANGE`, with `{permissionIds: []}` as its new value and no old
/// value, while the role keeps the permission it held: the row reads as "every
/// permission removed" for an edit that removed none.
///
/// **Today's behaviour, pinned. The fix inverts this test and renames it**: the
/// row then carries the stored role before and after, and says nothing of the
/// permissions this edit did not send.
///
/// [#1000]: https://github.com/sujanto-gaws/kelir/issues/1000
#[tokio::test]
async fn a_role_edit_audits_permission_ids_it_was_not_sent_issue_1000() {
    let mut walk = Walk::start().await;
    let permission = walk.permission("identity:user:read").await;
    let code = format!("ROLE-WALK-A-{}", walk.next());
    let role = id_of(
        &walk
            .post(
                "/api/v1/identity/roles",
                json!({ "roleCode": code, "name": "Buyers", "permissionIds": [permission] }),
            )
            .await,
    );

    let before = audit_ids(&walk.app).await;
    walk.put(
        &format!("/api/v1/identity/roles/{role}"),
        json!({ "name": "Senior buyers" }),
    )
    .await;
    let rows = rows_since(&walk.app, &before).await;

    let stored = walk
        .ok(
            Method::GET,
            &format!("/api/v1/identity/roles/{role}"),
            &walk.admin,
            None,
        )
        .await;
    assert_eq!(
        stored["permissions"].as_array().map(Vec::len),
        Some(1),
        "the role still holds its one permission: {stored}"
    );

    let row = the_one(&rows, "Role.Updated");
    assert_eq!(row.object_id, role);
    assert_eq!(row.action, "PERMISSION_CHANGE", "issue #1000: always this");
    assert_eq!(
        row.new_value,
        Some(json!({ "permissionIds": [] })),
        "issue #1000: the ids it was not sent, as none"
    );
    assert_eq!(row.old_value, None, "issue #1000: no old value");
}

/// **B, [#1001].** A tenant edit that sends only a status audits `name: null`
/// as its new value: `Tenant.Updated` is built from the request.
///
/// **Today's behaviour, pinned. The fix inverts this test and renames it**: the
/// row then carries the stored tenant, and no null for the name it kept.
///
/// [#1001]: https://github.com/sujanto-gaws/kelir/issues/1001
#[tokio::test]
async fn a_tenant_status_edit_audits_a_name_it_was_not_sent_issue_1001() {
    let mut walk = Walk::start().await;
    let tenant = walk.tenant().await;

    let before = audit_ids(&walk.app).await;
    walk.put(
        &format!("/api/v1/organization/tenants/{tenant}"),
        json!({ "status": "SUSPENDED" }),
    )
    .await;
    let rows = rows_since(&walk.app, &before).await;

    let name: String = sqlx::query_scalar("SELECT name FROM tenants WHERE id = $1")
        .bind(tenant)
        .fetch_one(&walk.app.pool)
        .await
        .expect("the tenant");
    assert_eq!(name, "A walked tenant", "the edit kept the name");

    let row = the_one(&rows, "Tenant.Updated");
    assert_eq!(row.object_id, tenant);
    assert_eq!(
        row.old_value,
        Some(json!({ "name": "A walked tenant", "status": "ACTIVE" }))
    );
    assert_eq!(
        row.new_value,
        Some(json!({ "name": null, "status": "SUSPENDED" })),
        "issue #1001: the name it was not sent, as null"
    );
}

/// A form with the two party fields a governed change carries.
async fn change_form(walk: &mut Walk) -> Uuid {
    let key = format!("walk-change-{}", walk.next());
    let form = id_of(
        &walk
            .post(
                "/api/v1/rad/forms",
                json!({
                    "formKey": key,
                    "title": "Supplier change",
                    "definition": {
                        "formId": key,
                        "version": "2.0.1",
                        "title": "Supplier change",
                        "components": [
                            { "id": "external-id", "role": "data", "type": "textfield",
                              "key": "externalId", "label": "External id",
                              "validation": { "type": "string" } },
                            { "id": "description", "role": "data", "type": "textfield",
                              "key": "description", "label": "Description",
                              "validation": { "type": "string" } }
                        ]
                    },
                }),
            )
            .await,
    );
    walk.post(&format!("/api/v1/rad/forms/{form}/publish"), json!({}))
        .await;
    form
}

/// **C, [#1002].** A change master-data governance raises parks the party,
/// and the approval that settles it applies its fields; **neither writes an
/// audit row on the party.** Its trail holds `Party.Created` and nothing after,
/// though its status moved twice and its fields once.
///
/// **Today's behaviour, pinned. The fix inverts this test and renames it**: the
/// raise and the settle each write a row on the party, read back through
/// `GET /api/v1/audit`.
///
/// [#1002]: https://github.com/sujanto-gaws/kelir/issues/1002
#[tokio::test]
async fn a_governed_change_writes_no_audit_row_on_the_party_issue_1002() {
    let mut walk = Walk::start().await;
    let reviewer = walk.approver().await;
    let reviewer_id = reviewer.actor.expect("a user");

    let key = format!("wf_walk_mdm_{}", walk.next());
    let definition = json!({
        "workflowKey": key,
        "version": "1.0.0",
        "name": "Master-data change approval",
        "initialState": "REVIEW",
        "states": [
            { "code": "REVIEW", "name": "Review the change",
              "mapsToDocumentStatus": "PENDING_APPROVAL",
              "task": { "taskDefinitionKey": "review", "taskName": "Review the change",
                        "assignment": { "assigneeType": "USER", "userId": reviewer_id.to_string() } } },
            { "code": "APPROVED", "name": "Approved", "mapsToDocumentStatus": "COMPLETED",
              "isFinal": true },
            { "code": "REJECTED", "name": "Rejected", "mapsToDocumentStatus": "REJECTED",
              "isFinal": true }
        ],
        "transitions": [
            { "from": "REVIEW", "to": "APPROVED", "action": "APPROVE",
              "allowedBy": { "assigneeType": "USER", "userId": reviewer_id.to_string() } },
            { "from": "REVIEW", "to": "REJECTED", "action": "REJECT",
              "allowedBy": { "assigneeType": "USER", "userId": reviewer_id.to_string() } }
        ]
    });
    let workflow = id_of(
        &walk
            .post(
                "/api/v1/workflow/definitions",
                json!({ "workflowKey": key, "name": "Master-data change approval",
                        "definition": definition }),
            )
            .await,
    );
    walk.post(
        &format!("/api/v1/workflow/definitions/{workflow}/publication"),
        json!({}),
    )
    .await;

    let form = change_form(&mut walk).await;
    let code = format!("WALK_MDM_{}", walk.next());
    let type_id = id_of(
        &walk
            .post(
                "/api/v1/document-types",
                json!({
                    "typeCode": code,
                    "name": code,
                    "formId": form,
                    "workflows": [{ "workflowDefinitionId": workflow }],
                    "targetEntityType": "PARTY",
                }),
            )
            .await,
    );
    walk.numbering_rule(type_id, &code).await;

    let party = walk.party().await;
    let document = id_of(
        &walk
            .post(
                "/api/v1/documents",
                json!({
                    "documentTypeId": type_id,
                    "title": "Change of supplier details",
                    "entityType": "PARTY",
                    "entityId": party,
                    "formData": { "externalId": "SUP-9002", "description": "approved by the manager" },
                }),
            )
            .await,
    );

    // The raise.
    walk.submit(document).await;
    let parked: String = sqlx::query_scalar("SELECT record_status FROM mdm_parties WHERE id = $1")
        .bind(party)
        .fetch_one(&walk.app.pool)
        .await
        .expect("the party");
    assert_eq!(parked, "PENDING_APPROVAL", "the submit parked the party");

    // The settle.
    let task = open_task(&walk.app, document).await;
    walk.ok(
        Method::POST,
        &format!("/api/v1/workflow/tasks/{task}/decision"),
        reviewer.token.as_deref().expect("a token"),
        Some(json!({ "action": "APPROVE" })),
    )
    .await;
    let (external, status): (Option<String>, String) =
        sqlx::query_as("SELECT external_id, record_status FROM mdm_parties WHERE id = $1")
            .bind(party)
            .fetch_one(&walk.app.pool)
            .await
            .expect("the party");
    assert_eq!(
        external.as_deref(),
        Some("SUP-9002"),
        "the approval applied the change"
    );
    assert_eq!(status, "ACTIVE", "and activated the party");

    let on_the_party: Vec<StoredRow> = stored_rows(&walk.app)
        .await
        .into_iter()
        .filter(|row| row.object_id == party)
        .collect();
    let events: Vec<&str> = on_the_party
        .iter()
        .map(|row| row.event_type.as_str())
        .collect();
    assert_eq!(
        events,
        vec!["Party.Created"],
        "issue #1002: the raise and the settle write nothing on the party"
    );
}

/// **D, [#1003].** The fifth wrong password locks the account, and the trail
/// holds five `Security.SignInFailed` rows, each `incorrect password`, and no
/// row of the lockout's own.
///
/// **Today's behaviour, pinned. The fix inverts this test and renames it**: the
/// lockout engaging then writes one row on the user, with its `locked_until`.
///
/// [#1003]: https://github.com/sujanto-gaws/kelir/issues/1003
#[tokio::test]
async fn a_lockout_engaging_writes_no_audit_row_of_its_own_issue_1003() {
    let mut walk = Walk::start().await;
    let (user, username) = walk.user(&[]).await;
    // An address of its own, so the rate limiter's bucket is this test's.
    let peer = SocketAddr::from(([192, 0, 2, 103], 41_003));

    let before = audit_ids(&walk.app).await;
    for _ in 0..5 {
        let refused = walk
            .app
            .send_from(
                peer,
                Method::POST,
                "/api/v1/auth/login",
                None,
                Some(json!({ "username": username, "password": "not-the-password-at-all" })),
            )
            .await;
        assert_eq!(refused.status, StatusCode::UNAUTHORIZED, "{}", refused.body);
    }
    let rows = rows_since(&walk.app, &before).await;

    let locked: Option<DateTime<Utc>> =
        sqlx::query_scalar("SELECT locked_until FROM users WHERE id = $1")
            .bind(user)
            .fetch_one(&walk.app.pool)
            .await
            .expect("the user");
    assert!(
        locked.is_some_and(|until| until > Utc::now()),
        "the fifth failure locked the account: {locked:?}"
    );

    let seen: Vec<(&str, Option<&str>)> = rows
        .iter()
        .map(|row| (row.event_type.as_str(), row.reason.as_deref()))
        .collect();
    assert_eq!(
        seen,
        vec![("Security.SignInFailed", Some("incorrect password")); 5],
        "issue #1003: the lock has no row of its own"
    );
    assert!(rows.iter().all(|row| row.object_id == user));
}

/// A listener that speaks INSTREAM and answers `reply` once the stream is in.
async fn scripted_clamd(reply: &'static str) -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a port");
    let port = listener.local_addr().expect("an address").port();

    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut sink = [0_u8; 8192];
                loop {
                    match socket.read(&mut sink).await {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {
                            let _ = socket.write_all(reply.as_bytes()).await;
                            let _ = socket.write_all(b"\0").await;
                            let _ = socket.shutdown().await;
                            return;
                        }
                    }
                }
                let _ = socket.shutdown().await;
            });
        }
    });

    port
}

async fn scan_status(app: &TestApp, attachment: Uuid) -> String {
    sqlx::query_scalar("SELECT virus_scan_status FROM attachments WHERE id = $1")
        .bind(attachment)
        .fetch_one(&app.pool)
        .await
        .expect("the scan status")
}

/// **E, [#1004].** The scan worker moves an attachment from `PENDING` to
/// `CLEAN`, and another to `INFECTED`, and writes no audit row for either.
///
/// **Today's behaviour, pinned. The fix inverts this test and renames it**: each
/// result then writes a row on the attachment, with no actor and the status
/// before and after.
///
/// [#1004]: https://github.com/sujanto-gaws/kelir/issues/1004
#[tokio::test]
async fn a_scan_result_writes_no_audit_row_issue_1004() {
    let mut walk = Walk::start().await;
    let document = walk.draft(None).await;

    let clean = walk.attachment(document).await;
    let before = audit_ids(&walk.app).await;
    let port = scripted_clamd("stream: OK").await;
    worker::pass(&walk.app.state, "127.0.0.1", port)
        .await
        .expect("a pass");
    assert_eq!(scan_status(&walk.app, clean).await, "CLEAN");
    let after_clean = rows_since(&walk.app, &before).await;

    let infected = walk.attachment(document).await;
    let before = audit_ids(&walk.app).await;
    let port = scripted_clamd("stream: Eicar-Test-Signature FOUND").await;
    worker::pass(&walk.app.state, "127.0.0.1", port)
        .await
        .expect("a pass");
    assert_eq!(scan_status(&walk.app, infected).await, "INFECTED");
    let after_infected = rows_since(&walk.app, &before).await;

    assert!(
        after_clean.is_empty(),
        "issue #1004: a clean result writes nothing; it wrote {}",
        listed(&after_clean)
    );
    assert!(
        after_infected.is_empty(),
        "issue #1004: an infected result writes nothing; it wrote {}",
        listed(&after_infected)
    );
}

// ---------------------------------------------------------------------------
// The other mutations that are not routes
// ---------------------------------------------------------------------------

/// **The bootstrap administrator is audited**, as `User.Created` with no actor:
/// nobody created it but the deployment.
#[tokio::test]
async fn the_bootstrap_administrator_is_audited_with_no_actor() {
    let walk = Walk::start().await;

    let created: Vec<StoredRow> = stored_rows(&walk.app)
        .await
        .into_iter()
        .filter(|row| row.object_id == walk.admin_id && row.event_type == "User.Created")
        .collect();

    assert_eq!(created.len(), 1, "{}", listed(&created));
    assert_eq!(created[0].object_type, "USER");
    assert_eq!(created[0].actor_user_id, None);
    assert!(!created[0].action.trim().is_empty());
}

/// **F, [#1005], out of scope for `v1.0.0`.** An approval lands in a state
/// whose service task takes an `AUTO` edge to `COMPLETED`. The decision writes
/// one row, `Workflow.TaskCompleted` on the human task: its transition ends at
/// the service state, and its `documentStatus` is where the automatic step
/// left the document. **The automatic step, and the system task it ran, have
/// no row**, and nothing is written on the document after its submit. The
/// steps between are in `workflow_history` only.
///
/// What is audited today, pinned. If #1005 is built, this test is inverted with
/// it.
///
/// [#1005]: https://github.com/sujanto-gaws/kelir/issues/1005
#[tokio::test]
async fn an_automatic_step_writes_no_audit_row_of_its_own_issue_1005() {
    let mut walk = Walk::start().await;
    let approver = walk.approver().await;

    let key = format!("wf_walk_auto_{}", walk.next());
    let definition = json!({
        "workflowKey": key,
        "version": "1.0.0",
        "name": "Stamped approval",
        "initialState": "SUBMITTED",
        "states": [
            { "code": "SUBMITTED", "name": "Awaiting approval",
              "mapsToDocumentStatus": "PENDING_APPROVAL",
              "task": { "taskDefinitionKey": "approve", "taskName": "Approve the request",
                        "assignment": { "assigneeType": "ROLE", "roleCode": APPROVER_ROLE } } },
            { "code": "STAMPING", "name": "Stamping",
              "mapsToDocumentStatus": "IN_REVIEW",
              "task": { "taskDefinitionKey": "stamp", "taskName": "Stamp it",
                        "taskType": "SERVICE_TASK",
                        "assignment": { "assigneeType": "ROLE", "roleCode": APPROVER_ROLE } } },
            { "code": "COMPLETED", "name": "Completed", "mapsToDocumentStatus": "COMPLETED",
              "isFinal": true }
        ],
        "transitions": [
            { "from": "SUBMITTED", "to": "STAMPING", "action": "APPROVE",
              "allowedBy": format!("ROLE:{APPROVER_ROLE}") },
            { "from": "STAMPING", "to": "COMPLETED", "action": "AUTO", "guards": [] }
        ],
        "variables": [
            { "key": "amount", "dataType": "NUMBER", "source": { "var": "formData.amount" } }
        ]
    });
    let workflow = id_of(
        &walk
            .post(
                "/api/v1/workflow/definitions",
                json!({ "workflowKey": key, "name": "Stamped approval", "definition": definition }),
            )
            .await,
    );
    walk.post(
        &format!("/api/v1/workflow/definitions/{workflow}/publication"),
        json!({}),
    )
    .await;

    let document = walk.draft(Some(workflow)).await;
    walk.submit(document).await;
    let task = open_task(&walk.app, document).await;

    let before = audit_ids(&walk.app).await;
    walk.ok(
        Method::POST,
        &format!("/api/v1/workflow/tasks/{task}/decision"),
        approver.token.as_deref().expect("a token"),
        Some(json!({ "action": "APPROVE" })),
    )
    .await;
    let rows = rows_since(&walk.app, &before).await;

    let stored: String = sqlx::query_scalar("SELECT status FROM documents WHERE id = $1")
        .bind(document)
        .fetch_one(&walk.app.pool)
        .await
        .expect("the document");
    assert_eq!(stored, "COMPLETED", "the automatic step ran");

    let automatic: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_history \
         WHERE document_id = $1 AND from_state = 'STAMPING' AND to_state = 'COMPLETED' \
         AND action = 'AUTO'",
    )
    .bind(document)
    .fetch_one(&walk.app.pool)
    .await
    .expect("the history");
    assert_eq!(automatic, 1, "the history holds the automatic step");

    assert_eq!(
        rows.len(),
        1,
        "issue #1005: the decision's row, and nothing for the automatic step: {}",
        listed(&rows)
    );
    let row = &rows[0];
    assert_eq!(row.event_type, "Workflow.TaskCompleted");
    assert_eq!(row.object_type, "WORKFLOW_TASK");
    assert_eq!(row.object_id, task);
    assert_eq!(row.actor_user_id, approver.actor);

    let new_value = row.new_value.clone().unwrap_or(Value::Null);
    assert_eq!(
        new_value["transition"],
        json!({ "from": "SUBMITTED", "action": "APPROVE", "to": "STAMPING" }),
        "the row names the decided transition only"
    );
    assert_eq!(
        new_value["documentStatus"],
        json!(stored),
        "the final status is in the decision's row"
    );
}

/// **The notification worker and the outbox worker write no audit row**, as
/// the product owner ruled on #960 (2026-11-16): what they deliver was audited
/// by the action that raised it, and their own record is `notification_logs`
/// and the outbox row's status.
///
/// Each pass is seen to do work, so the silence is the worker's and not an
/// empty queue's.
#[tokio::test]
async fn the_notification_and_outbox_workers_write_no_audit_row_as_ruled() {
    let mut walk = Walk::start().await;
    walk.approver().await;
    walk.awaiting_approval().await;

    let pending = |app: &TestApp| {
        let pool = app.pool.clone();
        async move {
            sqlx::query_scalar::<_, i64>(
                "SELECT count(*) FROM notifications WHERE status = 'PENDING'",
            )
            .fetch_one(&pool)
            .await
            .expect("the pending notifications")
        }
    };
    assert!(pending(&walk.app).await > 0, "the submit told the approver");

    let before = audit_ids(&walk.app).await;
    walk.app.deliver_notifications().await;
    let delivered = walk.app.deliver_outbox().await;
    let rows = rows_since(&walk.app, &before).await;

    assert_eq!(
        pending(&walk.app).await,
        0,
        "the notification pass sent what was pending"
    );
    assert!(delivered > 0, "the outbox pass claimed the transition");
    assert!(rows.is_empty(), "an exempt worker wrote {}", listed(&rows));
}

// ---------------------------------------------------------------------------
// AUD-003 — the approval history, read back through the trail
// ---------------------------------------------------------------------------

/// **Each approval is read back from `GET /api/v1/audit`**, by its task, and
/// says who decided, when, what, on which document and which transition. The
/// workflow's own history agrees with it step for step, and the last row's
/// `documentStatus` is the document's (NFR-AUD-003).
#[tokio::test]
async fn an_approval_is_read_back_from_the_audit_trail_one_decision_at_a_time() {
    let mut walk = Walk::start().await;

    let manager_role = fixtures::create_role_with_permissions(
        &walk.app.pool,
        SYSTEM_TENANT_ID,
        "WALK-MANAGER",
        &APPROVER_PERMISSIONS,
    )
    .await;
    let finance_role = fixtures::create_role_with_permissions(
        &walk.app.pool,
        SYSTEM_TENANT_ID,
        "WALK-FINANCE",
        &APPROVER_PERMISSIONS,
    )
    .await;
    let (manager, manager_name) = walk.user(&[manager_role]).await;
    let manager_token = walk.app.sign_in(&manager_name, PASSWORD).await;
    let (finance, finance_name) = walk.user(&[finance_role]).await;
    let finance_token = walk.app.sign_in(&finance_name, PASSWORD).await;

    let key = "wf_walk_two_step";
    let definition = json!({
        "workflowKey": key,
        "version": "1.0.0",
        "name": "Two-step approval",
        "initialState": "MANAGER_APPROVAL",
        "states": [
            { "code": "MANAGER_APPROVAL", "name": "Manager approval",
              "mapsToDocumentStatus": "PENDING_APPROVAL",
              "task": { "taskDefinitionKey": "manager_approval", "taskName": "Approve the request",
                        "assignment": { "assigneeType": "ROLE", "roleCode": "WALK-MANAGER" } } },
            { "code": "FINANCE_APPROVAL", "name": "Finance approval",
              "mapsToDocumentStatus": "PENDING_APPROVAL",
              "task": { "taskDefinitionKey": "finance_approval", "taskName": "Approve the spend",
                        "assignment": { "assigneeType": "ROLE", "roleCode": "WALK-FINANCE" } } },
            { "code": "COMPLETED", "name": "Completed", "mapsToDocumentStatus": "COMPLETED",
              "isFinal": true },
            { "code": "REJECTED", "name": "Rejected", "mapsToDocumentStatus": "REJECTED",
              "isFinal": true }
        ],
        "transitions": [
            { "from": "MANAGER_APPROVAL", "to": "FINANCE_APPROVAL", "action": "APPROVE",
              "allowedBy": "ROLE:WALK-MANAGER" },
            { "from": "MANAGER_APPROVAL", "to": "REJECTED", "action": "REJECT",
              "allowedBy": "ROLE:WALK-MANAGER" },
            { "from": "FINANCE_APPROVAL", "to": "COMPLETED", "action": "APPROVE",
              "allowedBy": "ROLE:WALK-FINANCE" },
            { "from": "FINANCE_APPROVAL", "to": "REJECTED", "action": "REJECT",
              "allowedBy": "ROLE:WALK-FINANCE" }
        ]
    });
    let workflow = id_of(
        &walk
            .post(
                "/api/v1/workflow/definitions",
                json!({ "workflowKey": key, "name": "Two-step approval", "definition": definition }),
            )
            .await,
    );
    walk.post(
        &format!("/api/v1/workflow/definitions/{workflow}/publication"),
        json!({}),
    )
    .await;

    let document = walk.draft(Some(workflow)).await;
    walk.submit(document).await;

    // The two decisions, each with its window.
    let mut decided = Vec::new();
    for (token, actor, from, to) in [
        (
            &manager_token,
            manager,
            "MANAGER_APPROVAL",
            "FINANCE_APPROVAL",
        ),
        (&finance_token, finance, "FINANCE_APPROVAL", "COMPLETED"),
    ] {
        let task = open_task(&walk.app, document).await;
        let started = now_in_microseconds();
        walk.ok(
            Method::POST,
            &format!("/api/v1/workflow/tasks/{task}/decision"),
            token,
            Some(json!({ "action": "APPROVE" })),
        )
        .await;
        decided.push((task, actor, from, to, started, Utc::now()));
    }

    let stored: String = sqlx::query_scalar("SELECT status FROM documents WHERE id = $1")
        .bind(document)
        .fetch_one(&walk.app.pool)
        .await
        .expect("the document");
    assert_eq!(stored, "COMPLETED");

    let history = walk
        .app
        .get(
            &format!("/api/v1/documents/{document}/workflow/history"),
            Some(&walk.admin),
        )
        .await;
    assert_eq!(history.status, StatusCode::OK, "{}", history.body);
    let steps = history.data().as_array().expect("the history").clone();
    assert_eq!(
        steps.len(),
        3,
        "the submit and two decisions: {}",
        history.body
    );

    let mut last_status = Value::Null;
    for (index, (task, actor, from, to, started, ended)) in decided.iter().enumerate() {
        let read = walk
            .app
            .get(
                &format!("/api/v1/audit?objectType=WORKFLOW_TASK&objectId={task}"),
                Some(&walk.admin),
            )
            .await;
        assert_eq!(read.status, StatusCode::OK, "{}", read.body);

        let rows: Vec<&Value> = read
            .data()
            .as_array()
            .expect("a page")
            .iter()
            .filter(|row| row["eventType"] == "Workflow.TaskCompleted")
            .collect();
        assert_eq!(rows.len(), 1, "one decision on task {task}: {}", read.body);
        let row = rows[0];

        assert_eq!(row["objectId"], json!(task.to_string()));
        assert_eq!(row["actorUserId"], json!(actor.to_string()), "who decided");
        assert_eq!(row["action"], "APPROVE", "what was decided");
        assert_eq!(row["valuesWithheld"], false, "{row}");

        let occurred: DateTime<Utc> = row["occurredAt"]
            .as_str()
            .and_then(|at| at.parse().ok())
            .unwrap_or_else(|| panic!("no time on {row}"));
        assert!(
            (*started..=*ended).contains(&occurred),
            "decided at {occurred}, outside {started} to {ended}"
        );

        let new_value = &row["newValue"];
        assert_eq!(new_value["documentId"], json!(document.to_string()));
        assert_eq!(
            new_value["transition"],
            json!({ "from": from, "action": "APPROVE", "to": to }),
            "which transition"
        );

        // The workflow's own history says the same, one step later than the
        // submit's row.
        let step = &steps[index + 1];
        assert_eq!(step["taskId"], json!(task.to_string()), "{step}");
        assert_eq!(step["fromState"], *from, "{step}");
        assert_eq!(step["toState"], *to, "{step}");
        assert_eq!(step["action"], "APPROVE", "{step}");
        let name = if index == 0 {
            &manager_name
        } else {
            &finance_name
        };
        assert_eq!(step["actorUsername"], json!(name), "{step}");

        last_status = new_value["documentStatus"].clone();
    }

    assert_eq!(
        last_status,
        json!(stored),
        "the trail's last word on the document is the document's status"
    );
}
