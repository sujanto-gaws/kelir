//! The one statement behind the action catalogue (§5.10, [#340]).
//!
//! The two conventions [`super`] states hold here: `tenant_id` comes from the
//! caller's claims, and `deleted_at IS NULL` keeps a retired action off a
//! screen.
//!
//! **`is_enabled` is a third filter and is not the same as a soft delete.** A
//! disabled action is one a deployment has switched off and expects to switch
//! back on; a deleted one is gone. Both are excluded here, and the column
//! exists so the first can be done without losing the configuration.
//!
//! [#340]: https://github.com/sujanto-gaws/kelir/issues/340

use sqlx::PgPool;
use uuid::Uuid;

use super::super::domain::action::{Action, ActionContext, ActionType};

/// A row as it is stored, before the caller's permissions narrow it.
///
/// **`required_permission` lives here and not on [`Action`]**, because it is
/// the question the service answers and not a field a renderer receives. The
/// two types are the same row on either side of that filter.
pub struct StoredAction {
    pub action: Action,
    pub required_permission: Option<String>,
}

/// Every enabled action configured for one context, in the order a screen
/// should show them.
///
/// **Ordered by `sort_order` then `action_key`**, the pairing `columns_of` uses
/// one file over: `sort_order` defaults to 0, so a deployment that configures
/// three actions and sets no order gets them alphabetically rather than in
/// whatever order the rows happen to come back — which is the difference
/// between a stable screen and one that reshuffles its buttons.
///
/// **A row whose `context` or `action_type` is outside its `CHECK` is dropped**
/// rather than surfaced, for the reason [`ActionContext::from_db`] gives: the
/// `CHECK` makes such a row unreachable through this API, so meeting one means
/// the constraint moved without this code, and a button whose behaviour cannot
/// be read is worse than a missing one.
pub async fn actions_for(
    pool: &PgPool,
    tenant_id: Uuid,
    context: ActionContext,
) -> Result<Vec<StoredAction>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"
        SELECT id, action_key, label, context, action_type, config_json,
               required_permission
        FROM rad_actions
        WHERE tenant_id = $1
          AND deleted_at IS NULL
          AND is_enabled
          AND context = $2
        ORDER BY sort_order, action_key
        "#,
        tenant_id,
        context.as_db(),
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .filter_map(|row| {
            Some(StoredAction {
                action: Action {
                    id: row.id,
                    action_key: row.action_key,
                    label: row.label,
                    context: ActionContext::from_db(&row.context)?,
                    action_type: ActionType::from_db(&row.action_type)?,
                    config: row.config_json,
                },
                required_permission: row.required_permission,
            })
        })
        .collect())
}
