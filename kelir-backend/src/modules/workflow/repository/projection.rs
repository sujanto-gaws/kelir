//! The `workflow_states` / `workflow_transitions` projections (§7.2, §7.3).
//!
//! **Regenerated whole on publish** (JWSS §9). Delete-then-insert rather than a
//! merge: `definition_json` is the authority, so a projected row that survives a
//! republish is a row the authority did not ask for.
//!
//! They are read by exactly two things. The **foreign key** on
//! `workflow_instances (workflow_definition_id, current_state)` reads
//! `workflow_states`, which is what makes [#175]'s AC4 a database fact rather
//! than a convention — and which is why writing the projection at publish is
//! what makes a definition startable at all. The **designer** reads both, and it
//! does not exist yet (FR-RAD-011, Sprints 14–16).
//!
//! **The engine reads neither.** It reads `definition_json`, because JWSS §1
//! calls that the single source of truth, and an engine reading the projection
//! would be executing a copy.
//!
//! [#175]: https://github.com/sujanto-gaws/kelir/issues/175

use uuid::Uuid;

use crate::modules::workflow::domain::Graph;

/// Replaces a definition's projected states and transitions.
///
/// In the publish transaction, so a definition is never `ACTIVE` with a
/// projection that does not match it — which would be an instance startable in a
/// state the definition no longer declares.
pub async fn replace(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    definition_id: Uuid,
    graph: &Graph,
    actor: Option<Uuid>,
) -> Result<(), sqlx::Error> {
    // Hard deletes. These rows are a projection rather than a record: nothing
    // references them but the foreign key, which is checked against the *new*
    // set in the same transaction, and a soft-deleted projection row would sit
    // in the unique index — which is total on purpose — and block the rewrite.
    sqlx::query!(
        "DELETE FROM workflow_transitions WHERE workflow_definition_id = $1",
        definition_id
    )
    .execute(&mut **transaction)
    .await?;

    sqlx::query!(
        "DELETE FROM workflow_states WHERE workflow_definition_id = $1",
        definition_id
    )
    .execute(&mut **transaction)
    .await?;

    for (order, state) in graph.states.iter().enumerate() {
        sqlx::query!(
            r#"
            INSERT INTO workflow_states
                (id, tenant_id, workflow_definition_id, state_code, name,
                 maps_to_document_status, is_initial, is_final, sort_order, created_by)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            "#,
            Uuid::now_v7(),
            tenant_id,
            definition_id,
            state.code,
            state.name,
            state.maps_to_document_status,
            state.code == graph.initial_state,
            state.is_final,
            order as i32,
            actor,
        )
        .execute(&mut **transaction)
        .await?;
    }

    for (order, transition) in graph.transitions.iter().enumerate() {
        // The rule is projected as JSON in its **normalized** object form, so a
        // designer reading the projection and an engine reading the definition
        // agree about what `"ROLE:X"` meant. Projecting the shorthand verbatim
        // would make the two disagree for exactly the definitions that use it.
        let allowed_by = match &transition.allowed_by {
            Some(rule) => serde_json::json!({
                "assigneeType": match rule.assignee_type {
                    crate::modules::workflow::domain::AssigneeType::User => "USER",
                    crate::modules::workflow::domain::AssigneeType::Role => "ROLE",
                    crate::modules::workflow::domain::AssigneeType::DepartmentRole => "DEPARTMENT_ROLE",
                    crate::modules::workflow::domain::AssigneeType::Owner => "OWNER",
                },
                "userId": rule.user_id,
                "roleCode": rule.role_code,
                "departmentScope": rule.department_scope,
            }),
            None => serde_json::json!({}),
        };

        sqlx::query!(
            r#"
            INSERT INTO workflow_transitions
                (id, tenant_id, workflow_definition_id, from_state, to_state, action,
                 allowed_by_json, condition_json, sort_order, created_by)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            "#,
            Uuid::now_v7(),
            tenant_id,
            definition_id,
            transition.from,
            transition.to,
            transition.action.as_db(),
            allowed_by,
            transition.condition,
            order as i32,
            actor,
        )
        .execute(&mut **transaction)
        .await?;
    }

    Ok(())
}
