//! Submitting a filled-in form (FR-RAD-010, FR-RAD-006, [#164]).
//!
//! **What is here is everything the re-evaluation is not**: the permission, the
//! tenant scope, the published check, the size bound, the write and the audit
//! record. The arithmetic is [`super::evaluation`]'s, and the split is the
//! deliverable rather than a preference — Sprint 9's
//! [#168](https://github.com/sujanto-gaws/kelir/issues/168) submits *through*
//! that re-evaluation inside the transaction that takes a document's number, and
//! that sentence is only true if the re-evaluation is callable without dragging
//! a submission row along with it (construction plan §6.2).
//!
//! **The refusals below all happen before any expression is evaluated, and the
//! order is not an accident.** Coding standard §2.9 calls that shape a *gate*:
//! a caller without the permission, a form that is not published, or a
//! definition in another tenant refuses at the top, and every mutation beneath
//! it then reports coverage that does not exist. It is the reason
//! `tests/rad_form_submissions.rs` builds a fixture that reaches the
//! re-evaluation rather than one that merely reaches the endpoint.
//!
//! [#164]: https://github.com/sujanto-gaws/kelir/issues/164

use serde_json::{json, Value};
use uuid::Uuid;

use super::super::domain::submission::{Submission, SubmitFormRequest, MAX_PAYLOAD_BYTES};
use super::super::domain::FormStatus;
use super::super::repository::form as form_repo;
use super::super::repository::submission::{self as repo, NewSubmission};
use super::super::FORM_SUBMIT;
use super::evaluation;
use crate::error::{AppError, ValidationDetail};
use crate::middleware::auth::Authenticated;
use crate::modules::audit::{self, AuditEntry};
use crate::state::AppState;

/// What the audit trail calls a submission (naming convention §7).
const OBJECT_TYPE: &str = "RAD_FORM_SUBMISSION";

/// Re-evaluates a submitted payload and stores the server's answer.
///
/// **The stored payload is never the submitted one.** JFSS S8.1 makes the
/// backend re-evaluate every `calculate` expression and overwrite the value
/// before persistence, and S10.2 does the same for every `conditional`. The
/// [operator-parity spike](../../../../../projects/spikes/01.%20JFSS%20Operator%20Parity.md)
/// measured the alternative: the Calculation Rule Registry §6.1 invoice stored
/// with a grand total of 0 in place of 42, nothing logged and nothing refused.
///
/// **The response carries the stored payload, read back from the row.** Two
/// reasons, and the second is the one worth stating: #135's rule makes a write
/// say what the row holds rather than what the request asked for, and #164 AC5
/// wants a caller to be able to see that the server's number differs from
/// theirs — *a form that changes your number without saying so is its own
/// defect*, and returning the answer is how it says so.
pub async fn submit_form(
    state: &AppState,
    caller: &Authenticated,
    form_id: Uuid,
    request: SubmitFormRequest,
) -> Result<Submission, AppError> {
    caller.require(FORM_SUBMIT)?;

    let tenant_id = caller.tenant_id();
    let actor = Some(caller.user_id());

    let form = form_repo::find_form(&state.pool, tenant_id, form_id)
        .await?
        .ok_or_else(|| AppError::not_found("Form definition"))?;

    // A draft is a form somebody is still writing, and its revision is not the
    // one any stored data may pin: editing a draft rewrites the definition in
    // place, so a payload validated against today's draft would be attached to
    // a revision that no longer means what it meant. Publishing is what freezes
    // a definition, which is exactly what makes a stored submission
    // interpretable later (§5.3).
    if form.status != FormStatus::Published {
        return Err(AppError::conflict(format!(
            "revision {} of `{}` is {:?}; only a published revision can be filled in",
            form.revision, form.form_key, form.status
        )));
    }

    bounded(&request.payload)?;

    // The whole of the Tamper-Proof Pattern, in one call. Nothing below reads
    // `request.payload` again.
    let payload = evaluation::secure_payload(&form.definition, &request.payload)
        .map_err(AppError::validation)?;

    let id = Uuid::now_v7();

    repo::insert_submission(
        &state.pool,
        &NewSubmission {
            id,
            tenant_id,
            form_id: form.id,
            form_revision: form.revision,
            payload_json: &payload,
            submitted_by: actor,
        },
    )
    .await?;

    // Read back before the record is written, so the record says what the row
    // holds rather than what the service believed it stored (#135). Those are
    // different claims, and this is the one endpoint whose whole purpose is to
    // make good on the difference.
    let stored = repo::find_submission(&state.pool, tenant_id, id)
        .await?
        .ok_or_else(|| AppError::Internal {
            source: anyhow::anyhow!("submission {id} vanished after it was written"),
        })?;

    audit::record_or_warn(
        &state.pool,
        AuditEntry {
            tenant_id,
            event_type: "RadForm.Submitted",
            action: "CREATE",
            object_type: OBJECT_TYPE,
            object_id: id,
            actor_user_id: actor,
            ip_address: None,
            reason: None,
            old_value: None,
            // The payload is deliberately not in the record, for the reason a
            // form definition is not: the trail keeps every version of every
            // record forever, and `rad_form_submissions` already keeps this
            // payload under a row that is never updated. What a reader of the
            // trail needs is that a submission happened, against which form and
            // which revision.
            new_value: Some(json!({
                "formId": stored.form_id,
                "formKey": form.form_key,
                "formRevision": stored.form_revision,
                "payloadKeys": payload_key_count(&stored.payload),
            })),
        },
    )
    .await;

    Ok(stored)
}

/// Refuses a payload larger than a form could legitimately collect.
///
/// Checked before the re-evaluation rather than after: the walk clones the
/// scope once per calculated field, so an unbounded payload is unbounded work
/// as well as an unbounded row.
fn bounded(payload: &Value) -> Result<(), AppError> {
    let bytes = payload.to_string().len();

    if bytes <= MAX_PAYLOAD_BYTES {
        return Ok(());
    }

    Err(AppError::validation(vec![ValidationDetail::new(
        "payload",
        "maxLength",
        "PAYLOAD_TOO_LARGE",
        format!("the submitted payload is {bytes} bytes and the limit is {MAX_PAYLOAD_BYTES}"),
    )]))
}

/// How many top-level keys the stored payload holds.
///
/// A count rather than the keys themselves: a key name is form configuration
/// and belongs in the definition's own audit records, while the number is what
/// tells a reader of the trail whether a submission was a whole form or a
/// fragment.
fn payload_key_count(payload: &Value) -> usize {
    payload.as_object().map(serde_json::Map::len).unwrap_or(0)
}
