//! Queries for `workflow_instances` and `workflow_variables` (§7.4, §7.5).

use chrono::{DateTime, Utc};
use sqlx::PgExecutor;
use uuid::Uuid;

use crate::modules::workflow::domain::instance::read_variable;
use crate::modules::workflow::domain::{
    InstanceOutcome, InstanceStatus, WorkflowInstance, WorkflowVariable,
};

/// The columns starting an instance writes.
pub struct NewInstance<'a> {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub instance_ref: &'a str,
    pub workflow_definition_id: Uuid,
    pub document_id: Uuid,
    pub business_key: Option<&'a str>,
    pub current_state: &'a str,
    pub started_by: Option<Uuid>,
}

/// An instance as the engine reads it under a lock: the few fields a decision
/// depends on, without the definition or the variables.
pub struct LockedInstance {
    pub id: Uuid,
    pub workflow_definition_id: Uuid,
    pub document_id: Uuid,
    pub current_state: String,
    pub status: InstanceStatus,
}

pub async fn insert_instance(
    transaction: &mut sqlx::PgTransaction<'_>,
    instance: &NewInstance<'_>,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        INSERT INTO workflow_instances
            (id, tenant_id, instance_ref, workflow_definition_id, document_id,
             business_key, status, current_state, started_by, created_by)
        VALUES ($1, $2, $3, $4, $5, $6, 'RUNNING', $7, $8, $8)
        "#,
        instance.id,
        instance.tenant_id,
        instance.instance_ref,
        instance.workflow_definition_id,
        instance.document_id,
        instance.business_key,
        instance.current_state,
        instance.started_by,
    )
    .execute(&mut **transaction)
    .await?;

    Ok(())
}

/// The live instance of a document, held for the rest of the transaction.
///
/// **`FOR UPDATE`, and it is taken before the task** — every path in this module
/// takes the instance first and then the task, which
/// [`super::super::service::engine`] states at the top. Two paths taking them in
/// opposite orders is a deadlock at exactly the concurrency the feature is for,
/// and it is a defect no single-threaded test can see.
pub async fn lock_instance(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    id: Uuid,
) -> Result<Option<LockedInstance>, sqlx::Error> {
    let row = sqlx::query!(
        r#"
        SELECT id, workflow_definition_id, document_id, current_state, status
        FROM workflow_instances
        WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL
        FOR UPDATE
        "#,
        tenant_id,
        id
    )
    .fetch_optional(&mut **transaction)
    .await?;

    Ok(row.map(|row| LockedInstance {
        id: row.id,
        workflow_definition_id: row.workflow_definition_id,
        document_id: row.document_id,
        current_state: row.current_state,
        status: InstanceStatus::from_db(&row.status),
    }))
}

/// One move of one instance, from one state to the next.
///
/// A record rather than five positional arguments: `from` and `to` are both
/// `&str`, so transposing them compiles — and a compare-and-swap whose two ends
/// are swapped is a guard that never matches, which presents as every
/// transition losing a race it was not in.
#[derive(Debug, Clone, Copy)]
pub struct StateMove<'a> {
    pub id: Uuid,
    /// The state the transition was **chosen against**, not the one the row
    /// happens to hold now. That is the whole mechanism.
    pub from: &'a str,
    pub to: &'a str,
    /// Whether `to` ends the process. **Separate from the outcome**, because a
    /// final state whose status maps to no outcome would otherwise leave the
    /// instance running in a state nothing can leave.
    pub final_state: bool,
    pub outcome: Option<InstanceOutcome>,
}

/// Moves an instance to its next state, **conditionally on it still being in
/// the one the transition was chosen against**.
///
/// One statement, compare-and-swap, `0` when it lost. `move_status`' shape one
/// module over, and for [record 03]'s reason: a check-then-act that lives in a
/// service is a rule somebody can step around by writing a second caller, and a
/// `WHERE` clause is a rule the database enforces on every caller there will
/// ever be.
///
/// [record 03]: ../../../../../projects/verifications/03.%20Sprint%206%20Surface%20Verification.md
pub async fn move_state(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    move_: &StateMove<'_>,
    actor: Option<Uuid>,
) -> Result<u64, sqlx::Error> {
    let StateMove {
        id,
        from,
        to,
        final_state,
        outcome,
    } = *move_;

    let affected = sqlx::query!(
        r#"
        UPDATE workflow_instances SET
            current_state = $4,
            -- **Entering a final state ends the instance, and the outcome is
            -- separate.** Which states end it is the definition's to say
            -- (`isFinal`), so this statement is told rather than deciding — and
            -- it is told the two things apart, because a final state whose
            -- `mapsToDocumentStatus` yields no outcome would otherwise leave the
            -- instance `RUNNING` in a state nothing can leave.
            status        = CASE WHEN $5 THEN 'COMPLETED' ELSE status END,
            outcome       = COALESCE($6, outcome),
            completed_at  = CASE WHEN $5 THEN now() ELSE completed_at END,
            updated_by    = $7,
            updated_at    = now()
        WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL AND current_state = $3
        "#,
        tenant_id,
        id,
        from,
        to,
        final_state,
        outcome.map(InstanceOutcome::as_db),
        actor,
    )
    .execute(&mut **transaction)
    .await?
    .rows_affected();

    Ok(affected)
}

/// The live instance of a document, if there is one.
///
/// Used by the seam to refuse a second one with a message, and by the document
/// workspace to show what is running. The partial unique index is what makes the
/// refusal true under concurrency; this read is what makes it a sentence a
/// person can act on.
pub async fn live_instance_of_document<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    document_id: Uuid,
) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query_scalar!(
        r#"
        SELECT id FROM workflow_instances
        WHERE tenant_id = $1 AND document_id = $2 AND deleted_at IS NULL
          AND status IN ('STARTED', 'RUNNING', 'SUSPENDED')
        "#,
        tenant_id,
        document_id
    )
    .fetch_optional(executor)
    .await
}

/// A row of `workflow_instances` joined to the revision it pins.
pub struct InstanceRow {
    pub id: Uuid,
    pub instance_ref: String,
    pub document_id: Uuid,
    pub workflow_definition_id: Uuid,
    pub workflow_key: String,
    pub workflow_name: String,
    pub definition_version: i32,
    pub status: String,
    pub current_state: String,
    pub outcome: Option<String>,
    pub business_key: Option<String>,
    pub started_by: Option<Uuid>,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

/// Reads an instance with the revision it is running.
///
/// **The version comes from the join, not from a column** ([#175] AC1 and the
/// [`domain::instance`][crate::modules::workflow::domain::instance] module
/// documentation): the reference *is* the pin, so storing the number beside it
/// would be a second copy of a fact this row already carries.
///
/// [#175]: https://github.com/sujanto-gaws/kelir/issues/175
pub async fn find_instance<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    id: Uuid,
) -> Result<Option<InstanceRow>, sqlx::Error> {
    let row = sqlx::query!(
        r#"
        SELECT i.id, i.instance_ref, i.document_id, i.workflow_definition_id,
               d.workflow_key, d.name AS workflow_name, d.version AS definition_version,
               i.status, i.current_state, i.outcome, i.business_key,
               i.started_by, i.started_at, i.completed_at
        FROM workflow_instances i
        JOIN workflow_definitions d ON d.id = i.workflow_definition_id
        WHERE i.tenant_id = $1 AND i.id = $2 AND i.deleted_at IS NULL
        "#,
        tenant_id,
        id
    )
    .fetch_optional(executor)
    .await?;

    Ok(row.map(|row| InstanceRow {
        id: row.id,
        instance_ref: row.instance_ref,
        document_id: row.document_id,
        workflow_definition_id: row.workflow_definition_id,
        workflow_key: row.workflow_key,
        workflow_name: row.workflow_name,
        definition_version: row.definition_version,
        status: row.status,
        current_state: row.current_state,
        outcome: row.outcome,
        business_key: row.business_key,
        started_by: row.started_by,
        started_at: row.started_at,
        completed_at: row.completed_at,
    }))
}

/// The instance of a document — live or finished, most recent first.
pub async fn instance_id_of_document<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    document_id: Uuid,
) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query_scalar!(
        r#"
        SELECT id FROM workflow_instances
        WHERE tenant_id = $1 AND document_id = $2 AND deleted_at IS NULL
        ORDER BY started_at DESC
        LIMIT 1
        "#,
        tenant_id,
        document_id
    )
    .fetch_optional(executor)
    .await
}

/// Writes an instance's variables (§7.5).
///
/// Insert-only rather than replace-the-set: variables are written once, at
/// instance start, from the definition's `source` expressions. When something
/// writes them *during* a process — a transition action, or a service task —
/// that caller will need an upsert, and it should add one rather than reuse
/// this and discover it silently drops what it did not name.
pub async fn insert_variables(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    instance_id: Uuid,
    variables: &[(String, String, String)],
    actor: Option<Uuid>,
) -> Result<(), sqlx::Error> {
    for (key, data_type, value) in variables {
        sqlx::query!(
            r#"
            INSERT INTO workflow_variables
                (id, tenant_id, workflow_instance_id, variable_key, variable_value,
                 data_type, created_by)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            "#,
            Uuid::now_v7(),
            tenant_id,
            instance_id,
            key,
            value,
            data_type,
            actor,
        )
        .execute(&mut **transaction)
        .await?;
    }

    Ok(())
}

pub async fn variables_of<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    instance_id: Uuid,
) -> Result<Vec<WorkflowVariable>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"
        SELECT variable_key, variable_value, data_type
        FROM workflow_variables
        WHERE tenant_id = $1 AND workflow_instance_id = $2 AND deleted_at IS NULL
        ORDER BY variable_key
        "#,
        tenant_id,
        instance_id
    )
    .fetch_all(executor)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| WorkflowVariable {
            value: read_variable(&row.variable_value, &row.data_type),
            key: row.variable_key,
            data_type: row.data_type,
        })
        .collect())
}

/// Assembles the API's view of an instance from a row and its variables.
pub fn to_instance(
    row: InstanceRow,
    current_state_name: String,
    variables: Vec<WorkflowVariable>,
) -> WorkflowInstance {
    WorkflowInstance {
        id: row.id,
        instance_ref: row.instance_ref,
        document_id: row.document_id,
        workflow_definition_id: row.workflow_definition_id,
        workflow_key: row.workflow_key,
        workflow_name: row.workflow_name,
        definition_version: row.definition_version,
        status: InstanceStatus::from_db(&row.status),
        current_state: row.current_state,
        current_state_name,
        outcome: row.outcome.as_deref().and_then(InstanceOutcome::from_db),
        business_key: row.business_key,
        started_by: row.started_by,
        started_at: row.started_at,
        completed_at: row.completed_at,
        variables,
    }
}
