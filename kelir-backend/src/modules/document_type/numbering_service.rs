//! Configuring a numbering rule, and allocating a number from it
//! (FR-DTYPE-004).
//!
//! [`allocate_in`] is the function this whole item exists for. Read it with
//! coding standard §2.5 open: the read, the decision and the write are one
//! transaction, and the lock is taken on the row the read consulted.

use chrono::Utc;
use uuid::Uuid;

use super::numbering::{
    render, scope_key, validate_set, AllocationContext, GapPolicy, NumberingRule, SequenceScope,
    SetNumberingRuleRequest,
};
use super::numbering_repository::{self as repo, NewRule};
use super::repository as type_repo;
use super::{TYPE_READ, TYPE_UPDATE};
use crate::error::{AppError, ValidationDetail};
use crate::middleware::auth::Authenticated;
use crate::modules::audit::{self, AuditEntry, ChangeSet};
use crate::state::AppState;

/// What the audit trail calls a numbering rule (naming convention §7).
const OBJECT_TYPE: &str = "DOCUMENT_TYPE_NUMBERING_RULE";

/// `document_type_numbering_rules.sequence_padding`'s own default.
const DEFAULT_PADDING: i32 = 6;

pub async fn get_rule(
    state: &AppState,
    caller: &Authenticated,
    document_type_id: Uuid,
) -> Result<NumberingRule, AppError> {
    caller.require(TYPE_READ)?;

    let tenant_id = caller.tenant_id();

    // The type is read first so that "no such type" and "no rule on this type"
    // are different answers. Both would be 404 otherwise, and a caller
    // debugging a mistyped id would be told the type has no numbering.
    type_repo::find_type(&state.pool, tenant_id, document_type_id)
        .await?
        .ok_or_else(|| AppError::not_found("Document type"))?;

    repo::find_rule(&state.pool, tenant_id, document_type_id)
        .await?
        .ok_or_else(|| AppError::not_found("Numbering rule"))
}

/// Sets — or replaces — a type's numbering rule.
pub async fn set_rule(
    state: &AppState,
    caller: &Authenticated,
    document_type_id: Uuid,
    request: SetNumberingRuleRequest,
) -> Result<NumberingRule, AppError> {
    caller.require(TYPE_UPDATE)?;

    let tenant_id = caller.tenant_id();
    let actor = Some(caller.user_id());

    type_repo::find_type(&state.pool, tenant_id, document_type_id)
        .await?
        .ok_or_else(|| AppError::not_found("Document type"))?;

    let before = repo::find_rule(&state.pool, tenant_id, document_type_id).await?;

    // A new rule starts in the bucket the clock is in now. A department-scoped
    // rule has no department at configuration time — it gets one per document —
    // so its bucket is settled on the first allocation instead.
    let context = AllocationContext {
        at: Utc::now(),
        department_id: None,
    };
    let key = if request.sequence_scope.needs_department() {
        String::new()
    } else {
        scope_key(request.sequence_scope, &context)
    };

    let issued = repo::highest_issued(&state.pool, tenant_id, document_type_id, &key).await?;

    validate_set(&request, issued)?;

    let mut transaction = state.pool.begin().await?;

    repo::replace_rule(
        &mut transaction,
        tenant_id,
        document_type_id,
        &NewRule {
            rule_template: request.rule_template.trim(),
            sequence_scope: request.sequence_scope.as_db(),
            sequence_padding: request.sequence_padding.unwrap_or(DEFAULT_PADDING),
            allow_gaps: request
                .gap_policy
                .unwrap_or(GapPolicy::Gapless)
                .allows_gaps(),
            sequence_key: &key,
            next_sequence: request.next_sequence.unwrap_or(1),
        },
        actor,
    )
    .await?;

    transaction.commit().await?;

    let after = repo::find_rule(&state.pool, tenant_id, document_type_id)
        .await?
        .ok_or_else(|| AppError::Internal {
            source: anyhow::anyhow!("numbering rule vanished after it was written"),
        })?;

    let mut changes = ChangeSet::new();
    changes.field(
        "ruleTemplate",
        &before.as_ref().map(|rule| rule.rule_template.clone()),
        &Some(after.rule_template.clone()),
    );
    changes.field(
        "sequenceScope",
        &before.as_ref().map(|rule| rule.sequence_scope),
        &Some(after.sequence_scope),
    );
    changes.field(
        "sequencePadding",
        &before.as_ref().map(|rule| rule.sequence_padding),
        &Some(after.sequence_padding),
    );
    changes.field(
        "gapPolicy",
        &before.as_ref().map(|rule| rule.gap_policy),
        &Some(after.gap_policy),
    );
    changes.field(
        "nextSequence",
        &before.as_ref().map(|rule| rule.next_sequence),
        &Some(after.next_sequence),
    );

    let (old_value, new_value) = changes.halves();

    audit::record_or_warn(
        &state.pool,
        AuditEntry {
            tenant_id,
            // The object is the *type*, not the rule row: a rule row is
            // replaced on every edit, so a trail keyed on it would be a
            // sequence of one-entry histories. "How was this type numbered
            // in March?" is the question a reader actually has.
            event_type: "DocumentTypeNumberingRule.Set",
            action: if before.is_some() { "UPDATE" } else { "CREATE" },
            object_type: OBJECT_TYPE,
            object_id: document_type_id,
            actor_user_id: actor,
            ip_address: None,
            reason: None,
            old_value: Some(old_value),
            new_value: Some(new_value),
        },
    )
    .await;

    Ok(after)
}

pub async fn clear_rule(
    state: &AppState,
    caller: &Authenticated,
    document_type_id: Uuid,
) -> Result<(), AppError> {
    caller.require(TYPE_UPDATE)?;

    let tenant_id = caller.tenant_id();
    let actor = Some(caller.user_id());

    type_repo::find_type(&state.pool, tenant_id, document_type_id)
        .await?
        .ok_or_else(|| AppError::not_found("Document type"))?;

    if repo::deactivate(&state.pool, tenant_id, document_type_id, actor).await? == 0 {
        return Err(AppError::not_found("Numbering rule"));
    }

    audit::record_or_warn(
        &state.pool,
        AuditEntry {
            tenant_id,
            event_type: "DocumentTypeNumberingRule.Cleared",
            action: "DELETE",
            object_type: OBJECT_TYPE,
            object_id: document_type_id,
            actor_user_id: actor,
            ip_address: None,
            reason: None,
            old_value: None,
            new_value: None,
        },
    )
    .await;

    Ok(())
}

/// Allocates the next number **inside the caller's transaction**.
///
/// This is the shape coding standard §2.5 governs, and the ordering is the
/// whole of it:
///
/// 1. `FOR UPDATE` on the rule row — the row the next step reads.
/// 2. Decide the bucket and the sequence *from what the lock returned*, not
///    from anything read earlier.
/// 3. Write the advanced counter in the same transaction.
///
/// A second caller reaching step 1 blocks until this transaction ends, so it
/// reads the advanced counter rather than the stale one. Without the lock both
/// read `41`, both render `…-000041`, and the second to commit is refused by
/// `uq_documents_tenant_id_document_number` — at submit time, after the work.
///
/// **The transaction is the caller's**, which is what makes a
/// [`GapPolicy::Gapless`] rule gapless: the caller's rollback rolls the
/// counter back with it. It is also what makes the lock expensive, because it
/// is held until the caller commits. [`allocate_committed`] is the other trade.
pub async fn allocate_in(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    document_type_id: Uuid,
    context: &AllocationContext,
) -> Result<String, AppError> {
    let rule = repo::lock_active_rule(transaction, tenant_id, document_type_id)
        .await?
        .ok_or_else(|| {
            AppError::validation(vec![ValidationDetail::new(
                "documentTypeId",
                "required",
                "NO_NUMBERING_RULE",
                "this document type has no active numbering rule, so a number \
                 cannot be assigned",
            )])
        })?;

    if rule.sequence_scope.needs_department() && context.department_id.is_none() {
        return Err(AppError::validation(vec![ValidationDetail::new(
            "requestedForDepartmentId",
            "required",
            "REQUIRED",
            "this document type numbers per department and per year, so a \
             document of it must name a department",
        )]));
    }

    let key = scope_key(rule.sequence_scope, context);

    // The bucket rolled over — a new year, a new month, a different department.
    // The counter restarts at 1 *in the new bucket*, and the old bucket's
    // counter is not kept: §6.3 stores one bucket per rule, so a document
    // back-dated into a closed bucket would restart it. Refused below rather
    // than silently re-issuing.
    let sequence = if key == rule.sequence_key {
        rule.next_sequence
    } else if is_earlier_bucket(rule.sequence_scope, &key, &rule.sequence_key) {
        return Err(AppError::validation(vec![ValidationDetail::new(
            "requestedAt",
            "range",
            "CLOSED_SEQUENCE_BUCKET",
            format!(
                "this type's sequence has moved on to `{}` and cannot go back to \
                 `{key}`; restarting a closed bucket re-issues numbers already \
                 given out",
                rule.sequence_key
            ),
        )]));
    } else {
        1
    };

    let number = render(
        &rule.rule_template,
        sequence,
        rule.sequence_padding,
        context,
    );

    repo::advance(transaction, rule.id, &key, sequence + 1).await?;

    Ok(number)
}

/// Allocates the next number in a transaction of its own, committing it.
///
/// The [`GapPolicy::AllowGaps`] path. The rule row is held for the length of
/// the allocation rather than the length of the caller's work, so concurrent
/// submissions of one type barely contend — and a number allocated to a
/// submission that then fails is gone, which is the trade the policy names.
///
/// **Not a shortcut around [`allocate_in`]**: a caller picks by reading the
/// rule's policy, which is what [`allocate`] does.
pub async fn allocate_committed(
    state: &AppState,
    tenant_id: Uuid,
    document_type_id: Uuid,
    context: &AllocationContext,
) -> Result<String, AppError> {
    let mut transaction = state.pool.begin().await?;
    let number = allocate_in(&mut transaction, tenant_id, document_type_id, context).await?;

    transaction.commit().await?;

    Ok(number)
}

/// Allocates a number the way this type's rule says to.
///
/// The caller passes its own transaction and gets it back unused if the rule
/// tolerates gaps — which is why the transaction is borrowed rather than
/// consumed. A caller that ignored the policy and always used its own
/// transaction would make every rule gapless and every submission serialise.
pub async fn allocate(
    state: &AppState,
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    document_type_id: Uuid,
    context: &AllocationContext,
) -> Result<String, AppError> {
    let policy = repo::find_rule(&state.pool, tenant_id, document_type_id)
        .await?
        .map(|rule| rule.gap_policy)
        .unwrap_or(GapPolicy::Gapless);

    match policy {
        GapPolicy::Gapless => allocate_in(transaction, tenant_id, document_type_id, context).await,
        GapPolicy::AllowGaps => {
            allocate_committed(state, tenant_id, document_type_id, context).await
        }
    }
}

/// Whether `candidate` is a bucket the sequence has already passed.
///
/// String comparison, and it works because every bucket key this module
/// produces sorts chronologically: `2026` before `2027`, `2026-08` before
/// `2026-09`. A `DEPARTMENT_YEAR` key is prefixed by the department, so two
/// different departments never compare as earlier or later — which is right,
/// they are parallel sequences and neither has passed the other.
fn is_earlier_bucket(scope: SequenceScope, candidate: &str, current: &str) -> bool {
    match scope {
        // One bucket, forever; it cannot be earlier than itself.
        SequenceScope::Global => false,
        SequenceScope::DepartmentYear => {
            let (candidate_department, candidate_year) = split_department(candidate);
            let (current_department, current_year) = split_department(current);

            candidate_department == current_department && candidate_year < current_year
        }
        _ => candidate < current,
    }
}

fn split_department(key: &str) -> (&str, &str) {
    key.rsplit_once(':').unwrap_or(("", key))
}
