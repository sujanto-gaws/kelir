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
//! 2. Begin. Read the document `FOR UPDATE`. Not a draft, refused (AC5).
//! 3. **Re-evaluate the payload at [`Strictness::Submit`]** — the full pipeline,
//!    `required` and unenforced rules refusing as **D-28** says.
//! 4. Allocate the number.
//! 5. Write the number, the status, `submitted_at` and the **server's** payload.
//! 6. Append the history row.
//! 7. Commit, then audit as a submit carrying the number.
//!
//! **Numbering before validating is the defect this order exists to avoid, and
//! on a `Gapless` rule it is worse than a burned number**: the counter is held
//! from the allocation to the commit, so a slow re-evaluation would serialise
//! every concurrent submit of that type behind a document that is about to be
//! refused.
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
use crate::modules::document_type::numbering::AllocationContext;
use crate::modules::document_type::numbering_service;
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

    let mut transaction = state.pool.begin().await?;

    let locked = repo::lock_document(&mut transaction, tenant_id, id)
        .await?
        .ok_or_else(|| AppError::not_found("Document"))?;

    // AC5: an already-submitted document is refused, not silently re-numbered.
    if !locked.status.is_editable() {
        return Err(AppError::conflict(format!(
            "this document is {} and only a draft can be submitted; submitting \
             again would take a second number for one document",
            locked.status.as_db()
        )));
    }

    let pinned = form::pinned_form_of(&mut transaction, tenant_id, &locked).await?;

    // Step 3, and it is before step 4 for the reason the module documentation
    // gives. `Strictness::Submit` is the full pipeline — this is the moment
    // `required` means something. The payload comes from the *locked* read, so
    // it is the one the status check was made against rather than one an edit
    // could have replaced in between.
    let form_data = super::document::secure(&pinned, &locked.form_data, Strictness::Submit)?;

    // Step 4. `allocate` reads the rule's own gap policy and picks: a `Gapless`
    // rule allocates in *this* transaction, so a rollback below rolls the
    // counter back with it; an `AllowGaps` rule allocates in one of its own and
    // leaves a hole if this fails. That is #158's decision and this call is what
    // honours it rather than re-taking it.
    let context = AllocationContext {
        at: Utc::now(),
        department_id: locked.requested_for_department_id,
    };

    let document_number = numbering_service::allocate(
        state,
        &mut transaction,
        tenant_id,
        locked.document_type_id,
        &context,
    )
    .await?;

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
