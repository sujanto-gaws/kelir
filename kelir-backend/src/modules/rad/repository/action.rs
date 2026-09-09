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
///
/// # What `list` selects ([#348])
///
/// **`Some(id)` is the tenant-wide actions plus that list's own; `None` is the
/// tenant-wide ones alone.** Not *every action of the context*, which is what
/// this paragraph claimed until it was read against the statement: with `$3`
/// null, `list_id = $3` is `NULL` rather than `true` for a scoped row, so the
/// row falls out of the `WHERE`.
///
/// **That is the behaviour to want, not an accident of three-valued logic to be
/// corrected.** A caller that did not say which list it is drawing must not be
/// handed buttons configured for one — offering a `LIST` action everywhere is
/// precisely the defect [#348] exists to close, and a `None` that meant
/// *everything* would reintroduce it for every caller that omits the parameter.
///
/// **An action with no `list_id` is the deployment saying *on every list***,
/// which is what every row meant before `0042` and why the migration needed no
/// data migration guessing which list an existing row had belonged to.
///
/// So `Some(id)` never sees fewer tenant-wide actions than `None` — naming a
/// list adds that list's own rows and removes none — which is what makes the
/// parameter safe to send from a renderer that cannot know whether the
/// deployment has scoped anything.
///
/// [#348]: https://github.com/sujanto-gaws/kelir/issues/348
pub async fn actions_for(
    pool: &PgPool,
    tenant_id: Uuid,
    context: ActionContext,
    list: Option<Uuid>,
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
          -- Tenant-wide rows always, plus this list's own when one was named.
          -- **With `$3` null the second disjunct is `NULL`, not `true`**, so a
          -- caller naming no list gets the tenant-wide rows alone — the note on
          -- `list` above is where that is argued for rather than observed.
          -- One statement rather than two: a second query differing by a clause
          -- is a second place for the tenant scope and the soft-delete filter
          -- to be forgotten.
          AND (list_id IS NULL OR list_id = $3)
        ORDER BY sort_order, action_key
        "#,
        tenant_id,
        context.as_db(),
        list,
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
