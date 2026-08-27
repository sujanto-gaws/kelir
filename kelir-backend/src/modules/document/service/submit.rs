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
//! [#158]: https://github.com/sujanto-gaws/kelir/issues/158
//! [#168]: https://github.com/sujanto-gaws/kelir/issues/168

use chrono::Utc;
use serde_json::json;
use uuid::Uuid;

use super::super::domain::{Document, DocumentStatus};
use super::super::repository::{self as repo, Submission};
use super::super::{DOCUMENT_SUBMIT, OBJECT_TYPE};
use super::form;
use crate::error::AppError;
use crate::middleware::auth::Authenticated;
use crate::modules::audit::{self, AuditEntry};
use crate::modules::document_type::numbering::{AllocationContext, GapPolicy};
use crate::modules::document_type::{numbering_repository, numbering_service};
use crate::modules::rad::service::evaluation::Strictness;
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

    refuse_unless_draft(subject.status)?;

    let context = AllocationContext {
        at: Utc::now(),
        department_id: subject.requested_for_department_id,
    };

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
    let committed_number = if policy.allows_gaps() {
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
    refuse_unless_draft(locked.status)?;

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
    let document_number = match committed_number {
        Some(number) => number,
        None => {
            numbering_service::allocate_in(
                &mut transaction,
                tenant_id,
                locked.document_type_id,
                &context,
            )
            .await?
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
            document_number: &document_number,
            form_data: &form_data,
            submitted_at,
        },
        actor,
    )
    .await?;

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

    // Step 6, in the same transaction. A document cannot end in a state its own
    // history does not explain.
    repo::record_transition(
        &mut transaction,
        tenant_id,
        id,
        Some(DocumentStatus::Draft),
        DocumentStatus::Submitted,
        actor,
        None,
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
            ip_address: None,
            reason: None,
            old_value: Some(json!({ "status": DocumentStatus::Draft })),
            new_value: Some(json!({
                "status": DocumentStatus::Submitted,
                "documentNumber": submitted.document_number,
                "submittedAt": submitted.submitted_at,
            })),
        },
    )
    .await;

    Ok(submitted)
}

/// Refuses a submission of a document that is not a draft (AC5).
///
/// Called twice on purpose: once on the unlocked read, so a gap-tolerant rule
/// does not burn a number on a document that was never submittable, and once
/// under the lock, which is the answer that counts.
fn refuse_unless_draft(status: DocumentStatus) -> Result<(), AppError> {
    if status.is_editable() {
        return Ok(());
    }

    Err(AppError::conflict(format!(
        "this document is {} and only a draft can be submitted; submitting again would take a second number for one document",
        status.as_db()
    )))
}
