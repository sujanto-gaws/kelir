//! The two statements the chain needs (§6.11, §6.12; [#339]).
//!
//! Both conventions the RAD repository states hold here too: `tenant_id` comes
//! from the caller's claims, and a read filters `deleted_at IS NULL`. The
//! execution log has neither `deleted_at` nor `updated_at` — §5 makes an
//! append-only table drop them, and this is one.
//!
//! [#339]: https://github.com/sujanto-gaws/kelir/issues/339

use serde_json::Value;
use uuid::Uuid;

use super::domain::{HandlerReference, RecentExecution, Registration, Source};

/// Every enabled registry entry for one hook, on one document type.
///
/// **A row with a null `document_type_id` applies to every type** — §6.11's own
/// comment says so — so the predicate is *this type or no type*, not *this
/// type*. Getting that backwards would make a tenant-wide policy a policy for
/// nobody, and the failure would be silent.
///
/// Ordered by priority then insertion, which is LHCS §3.1's *lower runs first;
/// ties resolve by registration order*. `created_at` is the registration order:
/// the table has no sequence, and a UUIDv7 id is chronological but is not a
/// promise the schema makes.
///
/// **A row whose `handler_reference` does not parse is dropped**, not carried.
/// §2's grammar is checked when an entry is registered; a row that stopped
/// matching it got there another way, and a chain that ran it would be running
/// something nothing has resolved.
pub async fn registry_chain(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    document_type_id: Uuid,
    hook_name: &str,
) -> Result<Vec<Registration>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"
        SELECT handler_reference, priority, config_json
        FROM document_lifecycle_hooks
        WHERE tenant_id = $1
          AND deleted_at IS NULL
          AND is_enabled
          AND hook_name = $2
          AND (document_type_id IS NULL OR document_type_id = $3)
        ORDER BY priority, created_at, id
        "#,
        tenant_id,
        hook_name,
        document_type_id,
    )
    .fetch_all(&mut **transaction)
    .await?;

    Ok(rows
        .into_iter()
        .filter_map(|row| {
            Some(Registration {
                hook: hook_name.to_owned(),
                handler: HandlerReference::parse(&row.handler_reference)?,
                priority: row.priority,
                config: row.config_json,
                source: Source::DocumentType,
            })
        })
        .collect())
}

/// One execution, recorded (LHCS §7).
///
/// **Every run, whatever its result and whatever its source** — §1.2's fourth
/// conformance point, and the reason this takes an `ERROR` result that
/// [`super::domain::HookResult`] cannot express: a handler that timed out
/// produced no result at all, and the log has to say that rather than record
/// the `REJECT` the engine synthesised from it.
///
/// Written in the caller's transaction, so a chain that ended in a rollback
/// leaves no trace of handlers that ran inside it. That is the right way round:
/// the log describes what happened to a document, and nothing happened to it.
pub struct Execution<'a> {
    pub tenant_id: Uuid,
    pub source: Source,
    /// `None` for a workflow-sourced handler, which has no registry row —
    /// §6.12's own column comment.
    pub hook_id: Option<Uuid>,
    /// `<workflowKey>@<revision>:<from>-><to>`, for a workflow-sourced handler.
    pub workflow_transition_ref: Option<&'a str>,
    pub document_id: Uuid,
    pub hook_name: &'a str,
    pub handler_reference: &'a str,
    pub result: &'a str,
    pub duration_ms: i32,
    pub error_message: Option<&'a str>,
}

pub async fn record(
    transaction: &mut sqlx::PgTransaction<'_>,
    execution: &Execution<'_>,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        INSERT INTO document_hook_executions
            (id, tenant_id, source, hook_id, workflow_transition_ref, document_id,
             hook_name, handler_reference, result, duration_ms, error_message)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
        "#,
        Uuid::now_v7(),
        execution.tenant_id,
        execution.source.as_db(),
        execution.hook_id,
        execution.workflow_transition_ref,
        execution.document_id,
        execution.hook_name,
        execution.handler_reference,
        execution.result,
        execution.duration_ms,
        execution.error_message,
    )
    .execute(&mut **transaction)
    .await
    .map(|_| ())
}

/// The document's promoted metadata, as LHCS §4's `metadata` field.
///
/// Read here rather than passed in, because the engine's transition path does
/// not otherwise hold it and a payload that omitted it would break §4's *the
/// full shape* promise for every handler.
pub async fn metadata_of(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    document_id: Uuid,
) -> Result<Value, sqlx::Error> {
    let rows = sqlx::query!(
        r#"
        SELECT metadata_key, metadata_value
        FROM document_metadata
        WHERE tenant_id = $1 AND document_id = $2 AND deleted_at IS NULL
        ORDER BY metadata_key
        "#,
        tenant_id,
        document_id,
    )
    .fetch_all(&mut **transaction)
    .await?;

    Ok(Value::Object(
        rows.into_iter()
            .map(|row| (row.metadata_key, Value::String(row.metadata_value)))
            .collect(),
    ))
}

/// One handler's most recent executions of one hook, newest first — what the
/// circuit breaker is derived from (ADR-0041 §2).
///
/// **Keyed by tenant, hook and handler reference, not by document or
/// definition.** A handler failing on every transition fails for a reason that
/// is not the document's, and a breaker counted per document would need five
/// documents to fail before it noticed one handler.
///
/// `cooled` is decided here, on the database's clock, because `executed_at` was
/// written by that clock: comparing it with this process's would make the
/// cool-down as long as the skew between two machines says.
///
/// Read through `idx_document_hook_executions_breaker` (`0045_outbox.sql`).
pub async fn recent_executions(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    hook_name: &str,
    handler_reference: &str,
    cool_down_seconds: f64,
    limit: i64,
) -> Result<Vec<RecentExecution>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"
        SELECT result = 'ERROR' AS "is_error!",
               executed_at <= now() - make_interval(secs => $4) AS "cooled!"
        FROM document_hook_executions
        WHERE tenant_id = $1 AND hook_name = $2 AND handler_reference = $3
        ORDER BY executed_at DESC, id DESC
        LIMIT $5
        "#,
        tenant_id,
        hook_name,
        handler_reference,
        cool_down_seconds,
        limit,
    )
    .fetch_all(&mut **transaction)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| RecentExecution {
            is_error: row.is_error,
            cooled: row.cooled,
        })
        .collect())
}

/// The tenant's own `ROLE-ADMIN`, the role a breaker reports to.
///
/// **By code within the tenant**: every tenant has its own row (D-18), so the
/// system tenant's id would name the wrong people everywhere else.
pub async fn administrator_role(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query_scalar!(
        r#"
        SELECT id
        FROM roles
        WHERE tenant_id = $1 AND role_code = $2 AND deleted_at IS NULL
        "#,
        tenant_id,
        crate::modules::identity::service::TENANT_ADMIN_ROLE_CODE,
    )
    .fetch_optional(&mut **transaction)
    .await
}

/// The document as a handler sees it at delivery (LHCS §4).
pub struct HookSubject {
    pub status: String,
    pub form_data: Value,
    pub document_type_id: Uuid,
    pub document_type_key: String,
}

/// Reads the document an after-chain runs on, **as it is now** — ADR-0041 §2:
/// the chain is invoked with the document at delivery, not at commit, because
/// the envelope may not carry form data (EES §4.2).
///
/// `None` for a document deleted since, which leaves the chain nothing to run
/// on.
pub async fn hook_subject(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    document_id: Uuid,
) -> Result<Option<HookSubject>, sqlx::Error> {
    let row = sqlx::query!(
        r#"
        SELECT d.status, d.form_data_json, d.document_type_id, t.type_code
        FROM documents d
        JOIN document_types t ON t.id = d.document_type_id AND t.tenant_id = d.tenant_id
        WHERE d.tenant_id = $1 AND d.id = $2 AND d.deleted_at IS NULL
        "#,
        tenant_id,
        document_id,
    )
    .fetch_optional(&mut **transaction)
    .await?;

    Ok(row.map(|row| HookSubject {
        status: row.status,
        form_data: row.form_data_json,
        document_type_id: row.document_type_id,
        document_type_key: row.type_code,
    }))
}
