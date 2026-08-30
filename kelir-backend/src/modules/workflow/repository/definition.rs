//! Queries for `workflow_definitions` (§7.1).
//!
//! **Every statement is tenant-scoped and soft-delete aware.** Not as a style
//! rule: a read that forgets `tenant_id` returns another tenant's approval
//! chain, and one that forgets `deleted_at IS NULL` binds a retired revision to
//! a document type.

use serde_json::Value;
use sqlx::{PgExecutor, PgPool};
use uuid::Uuid;

use crate::modules::workflow::domain::{
    WorkflowDefinition, WorkflowDefinitionStatus, WorkflowDefinitionSummary,
};

/// The columns a create writes.
///
/// `status` is absent: a new revision is always `DRAFT`, which is the column
/// default, and a create that could choose `ACTIVE` would be a way past the
/// permission publishing needs.
pub struct NewDefinition<'a> {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub workflow_key: &'a str,
    pub name: &'a str,
    pub description: Option<&'a str>,
    pub version: i32,
    pub jwss_version: &'a str,
    pub definition_json: &'a Value,
    pub initial_state: &'a str,
    pub created_by: Option<Uuid>,
}

/// What an update may change. `None` leaves the column alone.
pub struct DefinitionFields<'a> {
    pub name: Option<&'a str>,
    pub description: Option<&'a str>,
    pub definition_json: Option<&'a Value>,
    pub initial_state: Option<&'a str>,
    pub jwss_version: Option<&'a str>,
}

pub async fn count_definitions(pool: &PgPool, tenant_id: Uuid) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar!(
        "SELECT count(*) FROM workflow_definitions WHERE tenant_id = $1 AND deleted_at IS NULL",
        tenant_id
    )
    .fetch_one(pool)
    .await
    .map(|count| count.unwrap_or(0))
}

pub async fn list_definitions(
    pool: &PgPool,
    tenant_id: Uuid,
    limit: i64,
    offset: i64,
) -> Result<Vec<WorkflowDefinitionSummary>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"
        SELECT id, workflow_key, name, version, jwss_version, status, initial_state,
               created_at, updated_at
        FROM workflow_definitions
        WHERE tenant_id = $1 AND deleted_at IS NULL
        ORDER BY workflow_key, version DESC
        LIMIT $2 OFFSET $3
        "#,
        tenant_id,
        limit,
        offset
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| WorkflowDefinitionSummary {
            id: row.id,
            workflow_key: row.workflow_key,
            name: row.name,
            version: row.version,
            jwss_version: row.jwss_version,
            status: WorkflowDefinitionStatus::from_db(&row.status),
            initial_state: row.initial_state,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
        .collect())
}

pub async fn find_definition<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    id: Uuid,
) -> Result<Option<WorkflowDefinition>, sqlx::Error> {
    let row = sqlx::query!(
        r#"
        SELECT id, workflow_key, name, description, version, jwss_version, status,
               initial_state, definition_json, published_at, published_by,
               created_at, updated_at
        FROM workflow_definitions
        WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL
        "#,
        tenant_id,
        id
    )
    .fetch_optional(executor)
    .await?;

    Ok(row.map(|row| WorkflowDefinition {
        id: row.id,
        workflow_key: row.workflow_key,
        name: row.name,
        description: row.description,
        version: row.version,
        jwss_version: row.jwss_version,
        status: WorkflowDefinitionStatus::from_db(&row.status),
        initial_state: row.initial_state,
        definition: row.definition_json,
        published_at: row.published_at,
        published_by: row.published_by,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }))
}

/// The highest revision number under a key, **including soft-deleted ones**.
///
/// Including them is what stops the next revision colliding with a retired one:
/// the unique index is partial on `deleted_at`, so the number could be reused,
/// and reusing it would make "revision 3 of this workflow" ambiguous in an audit
/// trail that outlives both. `rad::repository::form` takes the same care for the
/// same reason.
pub async fn highest_version(
    pool: &PgPool,
    tenant_id: Uuid,
    workflow_key: &str,
) -> Result<Option<i32>, sqlx::Error> {
    sqlx::query_scalar!(
        r#"
        SELECT max(version) FROM workflow_definitions
        WHERE tenant_id = $1 AND workflow_key = $2
        "#,
        tenant_id,
        workflow_key
    )
    .fetch_one(pool)
    .await
}

pub async fn insert_definition<'e, E: PgExecutor<'e>>(
    executor: E,
    definition: &NewDefinition<'_>,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        INSERT INTO workflow_definitions
            (id, tenant_id, workflow_key, name, description, version, jwss_version,
             definition_json, initial_state, created_by)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
        "#,
        definition.id,
        definition.tenant_id,
        definition.workflow_key,
        definition.name,
        definition.description,
        definition.version,
        definition.jwss_version,
        definition.definition_json,
        definition.initial_state,
        definition.created_by,
    )
    .execute(executor)
    .await?;

    Ok(())
}

/// Edits a draft revision, and **only** while it is one.
///
/// `AND status = 'DRAFT'` repeats a check the service has already made, which
/// makes it a second line of defence: it guards a publish landing between the
/// service's read and this write. `workflow_definitions.rs`'s
/// `a_publish_that_lands_first_makes_the_edit_apply_to_nothing` reaches it by
/// holding the row's lock in one transaction while this statement blocks on it,
/// which is the technique coding standard §2.5 names.
pub async fn update_draft<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    id: Uuid,
    fields: &DefinitionFields<'_>,
    actor: Option<Uuid>,
) -> Result<u64, sqlx::Error> {
    let affected = sqlx::query!(
        r#"
        UPDATE workflow_definitions SET
            name            = COALESCE($3, name),
            description     = COALESCE($4, description),
            definition_json = COALESCE($5, definition_json),
            initial_state   = COALESCE($6, initial_state),
            jwss_version    = COALESCE($7, jwss_version),
            updated_by      = $8,
            updated_at      = now()
        WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL AND status = 'DRAFT'
        "#,
        tenant_id,
        id,
        fields.name,
        fields.description,
        fields.definition_json,
        fields.initial_state,
        fields.jwss_version,
        actor,
    )
    .execute(executor)
    .await?
    .rows_affected();

    Ok(affected)
}

/// Publishes a draft revision, conditionally on it still being one.
///
/// One statement, compare-and-swap: two callers who both read `DRAFT` produce
/// one update of one row and one update of none, and the second is told that
/// somebody else published it. Their name is on it, which is correct — the
/// second call published nothing.
pub async fn publish<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    id: Uuid,
    actor: Option<Uuid>,
) -> Result<u64, sqlx::Error> {
    let affected = sqlx::query!(
        r#"
        UPDATE workflow_definitions SET
            status       = 'ACTIVE',
            published_at = now(),
            published_by = $3,
            updated_by   = $3,
            updated_at   = now()
        WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL AND status = 'DRAFT'
        "#,
        tenant_id,
        id,
        actor,
    )
    .execute(executor)
    .await?
    .rows_affected();

    Ok(affected)
}

/// Holds a definition and reports whether it may be bound to a document type
/// ([#187](https://github.com/sujanto-gaws/kelir/issues/187) AC2).
///
/// `FOR SHARE` inside the caller's transaction, before the write it guards —
/// coding standard §2.5's rule, and `document_type::service::check_bindings`'
/// shape for the form binding beside it. A share lock blocks the retirement that
/// would invalidate the check and does not block a second type binding the same
/// workflow, which is the normal case rather than a rare one.
///
/// Returns the status rather than a boolean, because "no such definition" and
/// "that definition is a draft" are different refusals and the caller says so
/// differently.
pub async fn lock_bindable_definition(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    id: Uuid,
) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar!(
        r#"
        SELECT status FROM workflow_definitions
        WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL
        FOR SHARE
        "#,
        tenant_id,
        id
    )
    .fetch_optional(&mut **transaction)
    .await
}

pub async fn soft_delete<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    id: Uuid,
    actor: Option<Uuid>,
) -> Result<u64, sqlx::Error> {
    let affected = sqlx::query!(
        r#"
        UPDATE workflow_definitions
        SET deleted_at = now(), updated_by = $3, updated_at = now()
        WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL
        "#,
        tenant_id,
        id,
        actor
    )
    .execute(executor)
    .await?
    .rows_affected();

    Ok(affected)
}

/// Whether any instance is still running against this revision.
///
/// A retirement is refused over it, for `delete_type`'s reason one module over:
/// an instance *is* its definition — the definition says which transitions
/// exist and who may fire them — so retiring one under a running approval leaves
/// that approval unable to move. Deprecating the definition is how new documents
/// stop routing to it, and that is an update.
pub async fn has_live_instances<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    id: Uuid,
) -> Result<bool, sqlx::Error> {
    let found = sqlx::query_scalar!(
        r#"
        SELECT 1 AS "found!"
        FROM workflow_instances
        WHERE tenant_id = $1 AND workflow_definition_id = $2 AND deleted_at IS NULL
          AND status IN ('STARTED', 'RUNNING', 'SUSPENDED')
        LIMIT 1
        "#,
        tenant_id,
        id
    )
    .fetch_optional(executor)
    .await?;

    Ok(found.is_some())
}

/// Loads a definition to execute or to name, checked against the tenant that
/// pointed at it ([#260]).
///
/// # The invariant, which is not a call-site count
///
/// **Every caller passes an id it read from a row already scoped to that
/// tenant** — an instance row, or the document type's workflow binding. There
/// are five, and there were five when the comment here said there was one:
/// [`super::super::service::engine`], [`super::super::service::task`],
/// [`super::super::service::inbox`], [`super::super::service::instance`], and
/// `document::service::submit`. A count is the wrong thing to write down,
/// because it goes stale the first time somebody adds a caller and reads no
/// further than the signature — which is exactly what happened.
///
/// So the property is checked here instead of promised here.
///
/// # Why the row is still fetched unscoped, and then compared
///
/// **`WHERE id = $1 AND tenant_id = $2` would be worse**, and the original
/// reasoning for the unscoped read is why: a definition linked from another
/// tenant's row is a broken link, and a scoped `WHERE` turns it into *no row* —
/// indistinguishable from a definition that was never there, which the engine
/// reports as a binding to something that does not exist. The link is fetched,
/// the mismatch is named in the log with both tenants, and only then does the
/// answer become `None`. The outcome is the same and the diagnosis is not.
///
/// It cannot happen today: `workflow_definitions.id` is a foreign key from rows
/// that were themselves written under a tenant. This is what makes that
/// stay true rather than remain true by nobody having broken it.
///
/// # What was considered and not done
///
/// **A newtype only a tenant-scoped read can produce** — [#260] AC2 raises it.
/// Its producers span two modules, `document_type::repository::workflow_binding`
/// and this one's instance reads, so its constructor would have to be visible to
/// both; what the compiler would then keep is *somebody wrote a conversion*,
/// not *the id came from a scoped read*. The comparison below keeps the property
/// itself, and fails loudly rather than only in the places a future author
/// remembered to route through the type.
///
/// # A correction the finding did not catch
///
/// The comment this replaces opened with *"the scope is the instance's"*. On the
/// submit path there **is** no instance yet — `engine::start` is what creates
/// one, and the id comes from `document_type_workflows`, read under the
/// tenant. The scope is the row that named the definition, whichever row that
/// is.
///
/// [#260]: https://github.com/sujanto-gaws/kelir/issues/260
pub async fn definition_of_instance<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    workflow_definition_id: Uuid,
) -> Result<Option<ExecutableDefinition>, sqlx::Error> {
    let row = sqlx::query!(
        r#"
        SELECT definition_json, workflow_key, version, name, status,
               (tenant_id = $2) AS "in_tenant!"
        FROM workflow_definitions
        WHERE id = $1
        "#,
        workflow_definition_id,
        tenant_id
    )
    .fetch_optional(executor)
    .await?;

    let Some(row) = row else {
        return Ok(None);
    };

    if !row.in_tenant {
        // Loud, and with both ends of the broken link, because the caller
        // cannot tell this from a missing definition and will report it as one.
        tracing::error!(
            workflow_definition_id = %workflow_definition_id,
            tenant_id = %tenant_id,
            "a row in this tenant points at a workflow definition in another;              the definition was not loaded"
        );

        return Ok(None);
    }

    Ok(Some(ExecutableDefinition {
        definition_json: row.definition_json,
        workflow_key: row.workflow_key,
        version: row.version,
        name: row.name,
        status: row.status,
    }))
}

/// A definition as the engine executes it.
///
/// **It carries `status`**, which it did not until a re-read found
/// `engine::start`'s documentation claiming a check the code never made: #187
/// refuses a binding to anything but an `ACTIVE` definition, and a definition
/// can be deprecated *after* it is bound. Returning a tuple was what let the
/// field be forgotten — a fifth positional `String` beside three others is a
/// thing a reader skips.
pub struct ExecutableDefinition {
    pub definition_json: Value,
    pub workflow_key: String,
    pub version: i32,
    pub name: String,
    pub status: String,
}
