//! Submitting a draft, and taking its number in the same transaction
//! (FR-DOC-003, FR-DOC-004; [#168]).
//!
//! # Where the race actually is
//!
//! [#158] built the numbering rule and tested its allocator in isolation:
//! twenty-four concurrent allocations, two departments, buckets that do not
//! contend. **This is that allocation happening inside a real submit**, beside a
//! status change and #164's server-side re-evaluation, in one transaction that
//! commits whole or not at all.
//!
//! Two failures are specific to here and neither is covered by #158's tests:
//!
//! * **A number burned by a failed submit.** The number is allocated, the submit
//!   then fails re-evaluation, and either the transaction rolls back or the
//!   sequence has a hole. [`numbering_service::allocate`] behaves the way the
//!   rule's own [`GapPolicy`] decided — that is #158's decision and this item
//!   does not get to choose again.
//! * **A document that is numbered but not submitted, or submitted but not
//!   numbered.** Two writes that are not one transaction produce both, and both
//!   are unrecoverable by the user.
//!
//! # The order of operations, and why step 3 comes before step 4
//!
//! 1. `document:submit`, before anything is read.
//! 2. Read the document's status, type and department **on the pool**, and
//!    refuse early if it is not a draft.
//! 3. Read the rule's gap policy, and — **if it tolerates gaps** — allocate the
//!    number now, committed, with nothing else held.
//! 4. Begin. Read the document `FOR UPDATE`. Not a draft, refused (AC5) — this
//!    is the answer that counts, because step 2's read was unlocked.
//! 5. **Re-evaluate the payload at [`Strictness::Submit`]** — the full pipeline,
//!    `required` and unenforced rules refusing as **D-28** says.
//! 6. Allocate the number, if a gapless rule left it to be taken here.
//! 7. Write the number, the status, `submitted_at` and the **server's** payload.
//! 8. Append the history row.
//! 9. Commit, then audit as a submit carrying the number.
//!
//! **On a gapless rule the re-evaluation precedes the allocation, and that is
//! the whole item.** Numbering first burns a number on every refused
//! submission, and it is worse than a burned number: the counter is held from
//! the allocation to the commit, so a slow re-evaluation would serialise every
//! concurrent submit of that type behind a document that is about to be
//! refused.
//!
//! # Why a gap-tolerant rule allocates before the transaction, and what it costs
//!
//! **Because §2.5 of the [coding standard](../../../../../docs/standards/01.%20Coding%20Standard.md)
//! forbids the alternative.** A request MUST NOT hold more than one pooled
//! connection at a time, and `allocate_committed` opens a transaction of its own
//! — which is the policy's definition, not an implementation detail. Calling it
//! from inside this function's transaction is two connections per submit, and
//! that deadlocks a pool at the concurrency the pool can serve. It is
//! [#118](https://github.com/sujanto-gaws/kelir/issues/118)'s shape, and this
//! path had it twice: once in the policy *read*, fixed when a concurrency test
//! went red, and once here, found only by re-reading the whole path against the
//! rule. **A fix for a rule violation is checked against the rule, not against
//! the test that exposed it.**
//!
//! **What it costs: a gap-tolerant rule loses its number to a submission the
//! re-evaluation then refuses.** That is exactly the trade [#158]'s
//! `AllowGaps` names — *a number allocated to a submission that then fails is
//! gone* — so this restores the policy rather than weakening it. The earlier
//! arrangement kept the number, which was an accident of ordering that this
//! module had written down as a property, and it was bought with the violation
//! above.
//!
//! **Rejected: re-evaluating on the pool before allocating**, then writing under
//! a lock that re-checks the payload has not moved. It keeps the no-burn
//! behaviour and satisfies §2.5, and it buys that back with a new failure class
//! — a 409 a concurrent edit can raise on a submit that was going to succeed —
//! while making the two gap policies differ by less, which is the opposite of
//! what having two of them is for.
//!
//! # Sprint 10 adds one step, inside the transaction that already exists
//!
//! After the status is written and the history row appended, the type's workflow
//! binding is resolved and [`workflow::service::engine::start`][start] is called
//! — **in this transaction**, so a document can never be submitted without the
//! approval that was supposed to decide it, or carry an instance that decided a
//! submission which then rolled back. That is [#178] AC3.
//!
//! **A type with no binding submits and starts nothing** ([#187] AC4). Not every
//! document is approved, and a null binding is a valid configuration rather than
//! a missing one — so the shape below is *if a binding resolves*, never *the
//! binding must resolve*.
//!
//! `engine::start` takes `&mut PgTransaction` and never `AppState`, which is
//! stated at its own signature: this path already holds a numbering counter, and
//! **D-35** is what this project paid to learn that a second pooled connection
//! taken inside a transaction deadlocks at the concurrency the pool can serve.
//!
//! [#158]: https://github.com/sujanto-gaws/kelir/issues/158
//! [#168]: https://github.com/sujanto-gaws/kelir/issues/168
//! [#178]: https://github.com/sujanto-gaws/kelir/issues/178
//! [#187]: https://github.com/sujanto-gaws/kelir/issues/187
//! [start]: crate::modules::workflow::service::engine::start

use chrono::Utc;
use serde_json::{json, Value};
use uuid::Uuid;

use super::super::domain::{Document, DocumentStatus};
use super::super::repository::{self as repo, Submission};
use super::super::{DOCUMENT_SUBMIT, OBJECT_TYPE};
use super::form;
use crate::error::AppError;
use crate::middleware::auth::Authenticated;
use crate::modules::audit::{self, AuditEntry};
use crate::modules::document_type::numbering::{AllocationContext, GapPolicy};
use crate::modules::document_type::{
    numbering_repository, numbering_service, repository as document_type_repository,
};
use crate::modules::master_data::service::governance;
use crate::modules::rad::service::evaluation::Strictness;
use crate::modules::workflow::domain::{Graph, TransitionAction};
use crate::modules::workflow::repository::{
    definition as definition_repo, instance as instance_repo,
};
use crate::modules::workflow::service::assignment::AssignmentContext;
use crate::modules::workflow::service::engine;
use crate::state::AppState;

/// Submits a draft: one transaction, one number, one status change.
pub async fn submit_document(
    state: &AppState,
    caller: &Authenticated,
    id: Uuid,
) -> Result<Document, AppError> {
    caller.require(DOCUMENT_SUBMIT)?;

    let tenant_id = caller.tenant_id();
    let actor = Some(caller.user_id());

    // Steps 1 and 2, **before** the transaction opens, which is what coding
    // standard §2.5 means by resolving what the request points at first. On a
    // gap-tolerant rule the number is one of those things: it is committed
    // separately by design, and taking it while holding this request's own
    // transaction is the two-connection defect §2.5 forbids.
    let subject = repo::find_submission_subject(&state.pool, tenant_id, id)
        .await?
        .ok_or_else(|| AppError::not_found("Document"))?;

    refuse_unless_submittable(subject.status)?;

    // **A resubmission is a submit that takes no number** ([#183] AC5). The
    // document has one, and keeping it is the outcome return exists to preserve:
    // a returned document that came back with a new number would have lost its
    // place in every report, every reference and every conversation about it.
    //
    // Read from the status rather than from the number's presence, because the
    // two answer different questions and only one of them is the rule. A draft
    // with a number is not a state this codebase can produce, but writing the
    // condition as *has a number* would make it one the day something does.
    let resubmission = subject.status == DocumentStatus::Returned;

    let context = AllocationContext {
        at: Utc::now(),
        department_id: subject.requested_for_department_id,
    };

    // Resolved on the pool with the gap policy, for the same reason: it is a
    // fact about the *type*, not a thing this transaction has to hold. A binding
    // changed a microsecond later is a binding that applies to the next
    // document.
    let binding = document_type_repository::workflow_binding(
        &state.pool,
        tenant_id,
        subject.document_type_id,
    )
    .await?;

    let policy = numbering_repository::gap_policy(&state.pool, tenant_id, subject.document_type_id)
        .await?
        .unwrap_or(GapPolicy::Gapless);

    // **A gap-tolerant rule takes its number here, and therefore loses it if
    // anything below refuses.** That is the trade the policy names — a number
    // allocated to a submission that then fails is gone — and it is what makes
    // the two policies differ at all. The previous arrangement kept the number
    // through a failed re-evaluation, which was an accident of ordering bought
    // with a rule violation that deadlocks a pool at the concurrency the pool
    // can serve.
    //
    // **A resubmission allocates nothing, under either policy.** It is not that
    // the number would be discarded — it is that asking for one at all would
    // consume a value from the sequence for a document that already has its own.
    // On a gap-tolerant rule that is a permanent hole per correction round.
    let committed_number = if policy.allows_gaps() && !resubmission {
        Some(
            numbering_service::allocate_committed(
                state,
                tenant_id,
                subject.document_type_id,
                &context,
            )
            .await?,
        )
    } else {
        None
    };

    let mut transaction = state.pool.begin().await?;

    let locked = repo::lock_document(&mut transaction, tenant_id, id)
        .await?
        .ok_or_else(|| AppError::not_found("Document"))?;

    // AC5 again, and this time authoritatively: the read above was unlocked,
    // so a concurrent submit could have moved the document between the two.
    refuse_unless_submittable(locked.status)?;

    let pinned = form::pinned_form_of(&mut transaction, tenant_id, &locked).await?;

    // Step 3, and it is before step 4 for the reason the module documentation
    // gives. `Strictness::Submit` is the full pipeline — this is the moment
    // `required` means something. The payload comes from the *locked* read, so
    // it is the one the status check was made against rather than one an edit
    // could have replaced in between.
    let form_data = super::document::secure(&pinned, &locked.form_data, Strictness::Submit)?;

    // Step 4. A `Gapless` rule allocates in *this* transaction, so a rollback
    // below rolls the counter back with it. A gap-tolerant one already has its
    // number from above, committed, and a rollback leaves it as a hole. That is
    // #158's decision, honoured rather than re-taken.
    //
    // **A resubmission takes neither path**, and `assigned` stays `None` so the
    // statement below leaves the column alone.
    let assigned = match (resubmission, committed_number) {
        (true, _) => None,
        (false, Some(number)) => Some(number),
        (false, None) => Some(
            numbering_service::allocate_in(
                &mut transaction,
                tenant_id,
                locked.document_type_id,
                &context,
            )
            .await?,
        ),
    };

    // What the document will carry after this statement, which is the new number
    // on a first submit and the existing one on a resubmission. The workflow
    // below reads it as the business key and as `document.documentNumber`, and
    // a condition asking about the number must not be told `null` for a document
    // that has had one since its first submission.
    let document_number = match (&assigned, &subject.document_number) {
        (Some(number), _) => number.clone(),
        (None, Some(existing)) => existing.clone(),
        (None, None) => {
            return Err(AppError::Internal {
                source: anyhow::anyhow!(
                    "document {id} is being resubmitted and holds no number, which \
                     `mark_submitted` has assigned on every path that reaches RETURNED"
                ),
            })
        }
    };

    let submitted_at = Utc::now();

    // Step 5. The statement carries `status = 'DRAFT'` of its own, so two
    // callers who both passed the check above produce one update of one row and
    // one update of none.
    let affected = repo::mark_submitted(
        &mut transaction,
        tenant_id,
        id,
        &Submission {
            document_number: assigned.as_deref(),
            form_data: &form_data,
            submitted_at,
        },
        actor,
    )
    .await
    .map_err(colliding_number)?;

    if affected == 0 {
        // Somebody else submitted it while this transaction held its lock —
        // which the `FOR UPDATE` above makes unreachable through this service
        // and does not make unreachable through a future one. Rolling back
        // discards the number on a `Gapless` rule, which is the correct
        // outcome: nothing was submitted.
        return Err(AppError::conflict(
            "this document was submitted while this submission was being applied",
        ));
    }

    // **A governed master-data change parks its record here** (FR-MDM-010,
    // [#255](https://github.com/sujanto-gaws/kelir/issues/255), **D-55**), in
    // this transaction: the document becomes `PENDING` and the record becomes
    // `PENDING_APPROVAL` together, or neither does.
    //
    // **Whether this document is one is configuration** — the type's
    // `target_entity_type`, which `0015_document.sql` created for this and
    // nothing read until now — and the answer is `None` for every ordinary
    // document, which is all of them today. `master_data` is where the entity
    // is placed and where the record's table is known; nothing here learns
    // either, and the workflow engine below learns neither (#255 AC6).
    raise_governed_change(&mut transaction, tenant_id, id, &locked, &form_data, actor).await?;

    // Step 6, in the same transaction. A document cannot end in a state its own
    // history does not explain.
    // `locked.status` rather than a literal `DRAFT`: the row says where the
    // document actually came from, which is `RETURNED` on a resubmission. A
    // history claiming every submit began at a draft would erase the correction
    // round it is there to record.
    repo::record_transition(
        &mut transaction,
        tenant_id,
        id,
        Some(locked.status),
        DocumentStatus::Submitted,
        actor,
        None,
    )
    .await?;

    // Step 7 — the seam (#178, #187). In *this* transaction: a document that is
    // submitted and an approval that never started are two writes that must not
    // be able to disagree.
    //
    // **A resubmission moves the process it already has instead of starting
    // one** ([#183] AC5). The instance stayed running through the return — that
    // is what makes the loop a loop — so there is nothing to start, and starting
    // anyway would be refused by `uq_workflow_instances_one_live_per_document`
    // after the fact rather than by the code on purpose.
    let started = if resubmission {
        resubmit_workflow(
            &mut transaction,
            tenant_id,
            id,
            &subject,
            &engine::EvaluationContext {
                document: engine::document_facts(
                    DocumentStatus::Submitted,
                    subject.document_type_id,
                    Some(&document_number),
                ),
                form_data: form_data.clone(),
                variables: serde_json::json!({}),
            },
            actor,
        )
        .await?;

        None
    } else {
        start_workflow(
            &mut transaction,
            tenant_id,
            id,
            &subject,
            binding,
            &document_number,
            &engine::EvaluationContext {
                // The facts as they are **after** this submit: the status the
                // write above set and the number it took. A workflow condition
                // asking "is this submitted" must not be told about the draft it
                // was a statement ago.
                document: engine::document_facts(
                    DocumentStatus::Submitted,
                    subject.document_type_id,
                    Some(&document_number),
                ),
                form_data: form_data.clone(),
                variables: serde_json::json!({}),
            },
            actor,
        )
        .await?
    };

    crate::modules::activity::service::record(
        &mut transaction,
        &crate::modules::activity::service::Happening {
            tenant_id,
            document_id: Some(id),
            workflow_instance_id: None,
            task_id: None,
            attachment_id: None,
            comment_id: None,
            event_type: "Document.Submitted",
            category: crate::modules::activity::domain::EventCategory::Document,
            actor_user_id: actor,
            actor_name: Some(caller.username()),
            action_summary: "Submitted the document",
            details: serde_json::json!({ "documentNumber": document_number }),
        },
    )
    .await?;

    transaction.commit().await?;

    let submitted = repo::find_document(&state.pool, tenant_id, id)
        .await?
        .ok_or_else(|| AppError::Internal {
            source: anyhow::anyhow!("document {id} vanished after it was submitted"),
        })?;

    audit::record_or_warn(
        &state.pool,
        AuditEntry {
            tenant_id,
            // Its own event and its own action (AC6). A submit recorded as an
            // ordinary update would leave "who committed this requisition, and
            // what number did it get" answerable only by reading a payload.
            event_type: "Document.Submitted",
            action: "SUBMIT",
            object_type: OBJECT_TYPE,
            object_id: id,
            actor_user_id: actor,
            ip_address: caller.ip_address(),
            reason: None,
            // Where the document actually came from, which is `RETURNED` on a
            // resubmission ([#183]). A trail claiming every submit began at a
            // draft would erase the correction round — and an audit record that
            // disagrees with the row it describes is the defect this module
            // already fixed once for `status` below.
            old_value: Some(json!({ "status": subject.status })),
            new_value: Some(json!({
                // The document's status after the whole transaction, which is
                // the workflow's initial state when one started. Reporting
                // `SUBMITTED` unconditionally would make the trail disagree with
                // the row it describes.
                "status": submitted.status,
                "documentNumber": submitted.document_number,
                "submittedAt": submitted.submitted_at,
                // The instance, when a workflow started one. A submit that
                // routed and a submit that did not are different events, and an
                // auditor should not have to join two tables to tell them apart.
                "workflowInstanceRef": started.as_ref().map(|started| &started.instance_ref),
            })),
        },
    )
    .await;

    Ok(submitted)
}

/// Turns a colliding document number into a refusal an administrator can act on.
///
/// **Found by the Sprint 10 pass, against a configuration nobody had tried.**
/// `uq_documents_tenant_id_document_number` is **tenant-wide**, and a numbering
/// bucket is **per document type** — so two types whose templates render the
/// same string both issue `PR-2026-000001`, and the second submit violates the
/// index. Nothing mapped that violation, so it surfaced as a 500: an
/// `INTERNAL_ERROR` on a submit that was refused for a reason the product knows
/// exactly.
///
/// **The refusal names the template rather than the number**, because the number
/// is the symptom: two types sharing a template will collide again on the next
/// document and on every one after it, and what has to change is one of the two
/// templates. It is a 422 rather than a 409 — a 409 would say "try again", and
/// trying again produces the same collision forever.
///
/// **Not fixed by making the index per type.** The number is a *business*
/// identifier that people quote to each other and to suppliers, so two documents
/// in one tenant sharing one is the thing the index exists to prevent — the
/// collision is real and the only defect was the answer given for it.
fn colliding_number(error: sqlx::Error) -> AppError {
    match &error {
        sqlx::Error::Database(database) if database.is_unique_violation() => {
            AppError::validation(vec![crate::error::ValidationDetail::new(
                "documentNumber",
                "unique",
                "DUPLICATE_DOCUMENT_NUMBER",
                "this number is already held by another document in this tenant. A \
                 document number is unique tenant-wide and a numbering sequence is \
                 per document type, so two types whose rule templates render the same \
                 string will collide on every document — change one of the templates",
            )])
        }
        _ => error.into(),
    }
}

/// Refuses a submission of a document that is not submittable (AC5).
///
/// Called twice on purpose: once on the unlocked read, so a gap-tolerant rule
/// does not burn a number on a document that was never submittable, and once
/// under the lock, which is the answer that counts.
///
/// **A returned document is submittable** ([#183] AC5) and takes no second
/// number — the old refusal named that consequence, and it no longer applies to
/// the one status it now lets through.
///
/// [#183]: https://github.com/sujanto-gaws/kelir/issues/183
fn refuse_unless_submittable(status: DocumentStatus) -> Result<(), AppError> {
    if status.is_editable() {
        return Ok(());
    }

    Err(AppError::conflict(format!(
        "this document is {} and only a draft or a returned document can be \
         submitted; submitting a document already under approval would take a \
         second number for one document",
        status.as_db()
    )))
}

/// Sends a returned document back up the process it never left ([#183] AC5).
///
/// **The instance is still running.** A return moves it to the state the
/// definition's `RETURN` edge names and stops there; nothing completed it, which
/// is the whole difference between a return and a rejection. So this fires the
/// next transition on the instance the document already points at rather than
/// starting a second one.
///
/// # Why the action is `RESUBMIT` and not `SUBMIT`
///
/// They leave different states and mean different things.
/// [JWSS](../../../../../docs/schema/JSON%20Workflow%20Schema.md) §10's own
/// example has `DRAFT --SUBMIT--> MANAGER_APPROVAL` and
/// `RETURNED --RESUBMIT--> MANAGER_APPROVAL`, and a definition is entitled to
/// route the two differently — a correction that skips a step it has already
/// passed is the obvious case. Firing `SUBMIT` from `RETURNED` would ask for an
/// edge the specification does not put there.
///
/// # There is no task, and that is the shape rather than an omission
///
/// A `RETURNED` state declares none in JWSS's example: the document is with its
/// author, not in anybody's queue. So this cannot go through
/// `workflow::service::task::decide`, which is addressed by a task id — and the
/// authorization falls to the edge's own `allowedBy`, which is exactly what
/// [#226](https://github.com/sujanto-gaws/kelir/issues/226) built `permits` for
/// and the first caller to need it.
///
/// **`owner_user_id` is the document's `created_by`, not the caller.** They
/// coincide on a first submit and need not here: anybody holding
/// `document:submit` can reach this path, and `allowedBy: "OWNER"` on the
/// `RESUBMIT` edge has to refuse them. Reading the caller into the owner slot
/// would make that rule authorize everybody it was written to exclude.
///
/// [#183]: https://github.com/sujanto-gaws/kelir/issues/183
async fn resubmit_workflow(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    document_id: Uuid,
    subject: &repo::SubmissionSubject,
    evaluation: &engine::EvaluationContext,
    actor: Option<Uuid>,
) -> Result<(), AppError> {
    let Some(instance_id) = subject.workflow_instance_id else {
        // A document returned by the *manual* status route on a type that binds
        // no workflow. `may_move_to` allows `RETURNED -> SUBMITTED`, so this is
        // a real state and not a corruption: there is simply no process to move.
        return Ok(());
    };

    // Instance first, then task — `engine`'s ordering rule, on every path. There
    // is no open task to take second here, and taking the instance is still what
    // makes the state this reads the state the transition fires against.
    let instance = instance_repo::lock_instance(transaction, tenant_id, instance_id)
        .await?
        .ok_or_else(|| AppError::Internal {
            source: anyhow::anyhow!(
                "document {document_id} points at instance {instance_id}, which does not exist"
            ),
        })?;

    let definition = definition_repo::definition_of_instance(
        &mut **transaction,
        tenant_id,
        instance.workflow_definition_id,
    )
    .await?
    .ok_or_else(|| AppError::Internal {
        source: anyhow::anyhow!(
            "instance {instance_id} runs definition {} which does not exist",
            instance.workflow_definition_id
        ),
    })?;

    let graph = Graph::parse(&definition.definition_json, definition.version);

    engine::fire(
        transaction,
        tenant_id,
        instance.id,
        document_id,
        &graph,
        &instance.current_state,
        TransitionAction::Resubmit,
        actor,
        AssignmentContext {
            document_type_id: subject.document_type_id,
            owner_user_id: subject.created_by,
            requested_department_id: subject.requested_for_department_id,
            owner_department_id: None,
        },
        evaluation,
        // No task drove this and no comment came with it. The correction is the
        // document's own new payload, which the history's `to_state` and this
        // row's timestamp already place.
        engine::DecisionProvenance::default(),
    )
    .await?;

    Ok(())
}

/// Starts the approval a submitted document routes into, if its type binds one.
///
/// Returns `None` when the type binds nothing, which is [#187] AC4 — *a document
/// type with no workflow still works; submission completes without starting an
/// instance*. That is a valid configuration rather than a missing one, so the
/// absence is a value here rather than an error.
///
/// **The document's status after this is the workflow's, not `SUBMITTED`.** The
/// initial state's `mapsToDocumentStatus` is what the engine projects, which is
/// the whole of the seam: a workflow whose first state maps to `PENDING_APPROVAL`
/// leaves the document in `PENDING_APPROVAL` at the end of the same transaction
/// that submitted it. The `document_status_history` row above still says
/// `DRAFT -> SUBMITTED`, and that is accurate — the submit happened, and then
/// the process moved it.
///
/// [#187]: https://github.com/sujanto-gaws/kelir/issues/187
#[allow(
    clippy::too_many_arguments,
    reason = "the submit's own locals, passed rather than re-read: a struct here \
              would name them a second time and hide that every one of them comes \
              from the transaction this runs inside"
)]
async fn start_workflow(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    document_id: Uuid,
    subject: &repo::SubmissionSubject,
    binding: Option<(Uuid, bool)>,
    document_number: &str,
    evaluation: &engine::EvaluationContext,
    actor: Option<Uuid>,
) -> Result<Option<engine::Started>, AppError> {
    let Some((workflow_definition_id, has_condition)) = binding else {
        return Ok(None);
    };

    if has_condition {
        // See `document_type::repository::workflow_binding`: the column holds
        // the superseded string form, evaluating it is FR-WF-015's, and treating
        // it as unconditional would route a document by a rule nobody wrote. The
        // warning is what stops the skip being silent.
        tracing::warn!(
            document_id = %document_id,
            document_type_id = %subject.document_type_id,
            "the document type's workflow binding carries a condition expression, which \
             this release does not evaluate (FR-WF-015); no approval was started"
        );

        return Ok(None);
    }

    let started = engine::start(
        transaction,
        &engine::StartRequest {
            tenant_id,
            document_id,
            workflow_definition_id,
            business_key: Some(document_number),
            actor,
            context: AssignmentContext {
                document_type_id: subject.document_type_id,
                owner_user_id: actor,
                requested_department_id: subject.requested_for_department_id,
                owner_department_id: None,
            },
            evaluation,
        },
    )
    .await?;

    // FR-DOC-012, in the transaction that created the instance: a document never
    // has a process nothing points at, and an instance never exists with the
    // document unaware of it.
    repo::link_process_instance(transaction, tenant_id, document_id, started.instance_id).await?;

    Ok(Some(started))
}

/// Parks the master-data record a governed change is about, if this document is
/// one ([#255](https://github.com/sujanto-gaws/kelir/issues/255) AC1).
///
/// **Three things have to be true**, and a document that fails any of them is an
/// ordinary document: its type names an entity this build governs, the document
/// links a record, and the two agree about which kind of record it is.
///
/// **A type that governs and a document that links nothing is a refusal**, not a
/// silent pass. A change document with no record to change would be approved by
/// somebody and apply to nothing — which is the shape of the failure this whole
/// item exists to prevent, arriving through the configuration instead of through
/// the code.
async fn raise_governed_change(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    document_id: Uuid,
    locked: &repo::LockedDocument,
    form_data: &Value,
    actor: Option<Uuid>,
) -> Result<(), AppError> {
    let configured = document_type_repository::target_entity_type(
        &mut **transaction,
        tenant_id,
        locked.document_type_id,
    )
    .await?;

    let Some(entity) = governance::governed_entity(configured.as_deref()) else {
        return Ok(());
    };

    let (Some(linked_type), Some(entity_id)) = (locked.entity_type.as_deref(), locked.entity_id)
    else {
        return Err(AppError::conflict(
            "this document type routes master-data changes through approval, so a document of \
             it has to name the record it changes",
        ));
    };

    if governance::governed_entity(Some(linked_type)) != Some(entity) {
        return Err(AppError::conflict(
            "this document names a different kind of record from the one its type governs",
        ));
    }

    governance::raise(
        transaction,
        tenant_id,
        entity,
        entity_id,
        document_id,
        form_data,
        actor,
    )
    .await
}
