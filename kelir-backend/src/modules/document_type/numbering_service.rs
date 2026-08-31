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
        },
        actor,
    )
    .await?;

    // `nextSequence` seeds a bucket, and only when it was asked for: writing one
    // unconditionally would create a bucket for a rule nobody has allocated
    // from, and `find_rule` would then report a sequence that has not started.
    //
    // No scope check here. `validate_set` has already refused `nextSequence` on
    // a `DEPARTMENT_YEAR` rule — there is no bucket to seed, because the
    // department that identifies one arrives with the first document — so a
    // second guard would be a branch no test can reach, which is the shape
    // [#206](https://github.com/sujanto-gaws/kelir/issues/206) is about.
    if let Some(next_sequence) = request.next_sequence {
        repo::seed_bucket(
            &mut transaction,
            tenant_id,
            document_type_id,
            &key,
            next_sequence,
            actor,
        )
        .await?;
    }

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
            ip_address: caller.ip_address(),
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
            ip_address: caller.ip_address(),
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
/// This is the shape coding standard §2.5 governs, and since
/// `0020_numbering_buckets.sql` the whole of it is one statement:
///
/// 1. Read the rule — the *format*, which no allocation writes.
/// 2. Decide the bucket from the context.
/// 3. Insert-or-advance that bucket, and take the number it returns.
///
/// Step 3 has no read to race. Two callers in the same bucket are serialised
/// by `uq_document_type_sequence_buckets_type_key`; two callers in *different*
/// buckets touch different rows and do not contend at all — which the previous
/// shape could not manage, because it kept every bucket in the rule row and so
/// made two departments fight over one counter and then lose it.
///
/// **The transaction is the caller's**, which is what makes a
/// [`GapPolicy::Gapless`] rule gapless: the caller's rollback rolls the
/// counter back with it, and the bucket row stays locked until the caller
/// commits. [`allocate_committed`] is the other trade.
pub async fn allocate_in(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    document_type_id: Uuid,
    context: &AllocationContext,
) -> Result<String, AppError> {
    let rule = repo::find_active_rule(&mut **transaction, tenant_id, document_type_id)
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

    // A bucket the sequence has already passed is still refused, and the reason
    // is no longer the one §6.3 gave. Under one-bucket-per-rule, reaching back
    // *restarted* the counter and re-issued numbers documents already held.
    // Buckets persist now, so reaching back would resume the old bucket
    // correctly — the refusal survives because a closed period should not gain
    // new numbers, which is a policy rather than a safety property. Recorded in
    // **D-21** as a question for the product owner rather than settled here.
    //
    // `comparison_prefix` is what makes one comparison serve every scope: it
    // restricts the group to keys that genuinely succeed one another, so two
    // departments — which are parallel, not successive — are never compared.
    let prefix = comparison_prefix(rule.sequence_scope, context);

    if let Some(furthest) =
        repo::furthest_key(&mut **transaction, tenant_id, document_type_id, &prefix).await?
    {
        if key < furthest {
            return Err(AppError::validation(vec![ValidationDetail::new(
                "requestedAt",
                "range",
                "CLOSED_SEQUENCE_BUCKET",
                format!(
                    "this type's sequence has moved on to `{furthest}` and cannot \
                     go back to `{key}`; a closed period does not take new numbers"
                ),
            )]));
        }
    }

    let sequence = repo::allocate_bucket(transaction, tenant_id, document_type_id, &key).await?;

    Ok(render(
        &rule.rule_template,
        sequence,
        rule.sequence_padding,
        context,
    ))
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
    // **On the caller's transaction, not on the pool.** Reading the policy from
    // `state.pool` takes a second connection while the caller holds a
    // transaction on the first, so a submit costs two — and twenty-four
    // concurrent submits against a five-connection pool deadlock, every task
    // holding one and waiting for one nobody will release. That is #118's shape
    // and it survived here because no caller used `allocate` under load until
    // Sprint 9's submit did.
    let policy = repo::gap_policy(&mut **transaction, tenant_id, document_type_id)
        .await?
        .unwrap_or(GapPolicy::Gapless);

    match policy {
        GapPolicy::Gapless => allocate_in(transaction, tenant_id, document_type_id, context).await,
        GapPolicy::AllowGaps => {
            allocate_committed(state, tenant_id, document_type_id, context).await
        }
    }
}

/// The set of buckets a candidate key may be compared against, as a key prefix.
///
/// **This is what `is_earlier_bucket` was, expressed as data instead of as a
/// branch**, and the change is the fix for #200. That function answered "is
/// this key earlier?" and got the department case right — two departments are
/// parallel sequences, and neither has passed the other — while the caller
/// treated "not earlier" as "start again at 1". The judgement was sound and the
/// action behind it was wrong.
///
/// Restricting the comparison group instead means the caller has one rule for
/// every scope: within a group, keys succeed one another and `<` means "already
/// passed"; across groups there is no comparison to make, because each group has
/// its own row and its own counter.
///
/// * `GLOBAL` — one key, `""`. Nothing precedes it.
/// * `YEAR`, `MONTH` — every key is in one succession, so the group is all of
///   them and the prefix is empty.
/// * `DEPARTMENT_YEAR` — one succession *per department*, so the group is the
///   department's own keys and the prefix is `<department>:`.
fn comparison_prefix(scope: SequenceScope, context: &AllocationContext) -> String {
    match scope {
        SequenceScope::DepartmentYear => context
            .department_id
            .map(|id| format!("{id}:"))
            // Unreachable: a department-scoped allocation with no department is
            // refused above. Matching nothing rather than everything is the
            // safe reading if that ever stops being true.
            .unwrap_or_else(|| "NO-DEPARTMENT:".to_owned()),
        _ => String::new(),
    }
}
