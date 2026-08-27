//! Queries for `rad_form_submissions` (§5.14).
//!
//! The two conventions this module states once hold here too: every statement
//! filters `tenant_id`, taken from the caller's claims rather than from the
//! request, and every read filters `deleted_at IS NULL`. Nothing here writes
//! `deleted_at` — a submission is a record of something that happened — and the
//! reads filter it anyway, so the day a retention policy sets one the reads are
//! already right.

use serde_json::Value;
use sqlx::PgExecutor;
use uuid::Uuid;

use crate::modules::rad::domain::submission::Submission;

/// The columns a submission writes.
///
/// `payload_json` is the **secure** payload — the one
/// [`crate::modules::rad::service::evaluation`] produced. There is no column
/// for the submitted one, and that is the design: storing what the client sent
/// beside what the server computed would create a second answer for anybody
/// who reached for the wrong one.
pub struct NewSubmission<'a> {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub form_id: Uuid,
    pub form_revision: i32,
    pub payload_json: &'a Value,
    pub submitted_by: Option<Uuid>,
}

/// Writes one submission.
///
/// Takes an executor rather than the pool so Sprint 9's
/// [#168](https://github.com/sujanto-gaws/kelir/issues/168) can call it inside
/// the transaction that takes the document's number — a number burned by a
/// failed submit and a document numbered but not submitted are both
/// unrecoverable by the user, so the two writes commit whole or not at all.
pub async fn insert_submission<'e, E: PgExecutor<'e>>(
    executor: E,
    submission: &NewSubmission<'_>,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        INSERT INTO rad_form_submissions
            (id, tenant_id, form_id, form_revision, payload_json, created_by, updated_by)
        VALUES ($1, $2, $3, $4, $5, $6, $6)
        "#,
        submission.id,
        submission.tenant_id,
        submission.form_id,
        submission.form_revision,
        submission.payload_json,
        submission.submitted_by,
    )
    .execute(executor)
    .await?;

    Ok(())
}

pub async fn find_submission<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    id: Uuid,
) -> Result<Option<Submission>, sqlx::Error> {
    let row = sqlx::query!(
        r#"
        SELECT id, form_id, form_revision, payload_json, submitted_at,
               created_by, created_at, updated_at
        FROM rad_form_submissions
        WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL
        "#,
        tenant_id,
        id
    )
    .fetch_optional(executor)
    .await?;

    Ok(row.map(|row| Submission {
        id: row.id,
        form_id: row.form_id,
        form_revision: row.form_revision,
        payload: row.payload_json,
        submitted_at: row.submitted_at,
        submitted_by: row.created_by,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }))
}
