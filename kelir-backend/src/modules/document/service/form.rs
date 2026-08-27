//! Finding the definition a document is validated against.
//!
//! Two questions that look like one and are not:
//!
//! * **At creation**, the definition is *whatever the type binds right now*, and
//!   the answer is written onto the document as `form_id`. That is the pin, and
//!   [`super::document`]'s module documentation says what it buys.
//! * **At every write afterwards**, the definition is *whatever this document
//!   pinned*, and the type's current binding is irrelevant. A document that
//!   revalidated against the type's binding would be re-validated against a
//!   definition nobody filled it in against the moment an administrator
//!   published a revision — which is exactly the failure **D-30** and
//!   `guard_rebinding` exist to prevent, arriving through the other door.
//!
//! Both are here rather than in `document.rs` because both are also what the
//! submit needs, and a second copy of "which definition governs this document"
//! is a second answer to the question the whole pin is about.

use uuid::Uuid;

use super::super::repository::LockedDocument;
use crate::error::AppError;

/// The definition a document is held to, and the revision it was pinned from.
pub struct PinnedForm {
    /// `None` when the type binds no form. §6.2 permits that, and
    /// [`super::document::secure`] is what decides what it means for data.
    pub form_id: Option<Uuid>,
    pub definition: Option<serde_json::Value>,
}

/// The form a type binds **right now**, holding the type row.
///
/// `FOR UPDATE` on `document_types`, and the lock is doing two jobs. It makes
/// the binding this creation is about to pin stable for the length of the
/// transaction, and it serialises the creation against a concurrent rebinding:
/// `document_type::service::guard_rebinding` takes `FOR UPDATE` on the same row
/// before it counts unpinned documents, so a document created here is either
/// counted by that guard or created after it — never in between.
///
/// Returns `None` when there is no such type in this tenant, which the caller
/// turns into a 422 naming the field rather than a 404 about the document.
pub async fn lock_pinned_form(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    document_type_id: Uuid,
) -> Result<Option<PinnedForm>, AppError> {
    let Some(row) = sqlx::query!(
        r#"
        SELECT form_id
        FROM document_types
        WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL
        FOR UPDATE
        "#,
        tenant_id,
        document_type_id
    )
    .fetch_optional(&mut **transaction)
    .await?
    else {
        return Ok(None);
    };

    Ok(Some(PinnedForm {
        form_id: row.form_id,
        definition: definition_of(transaction, tenant_id, row.form_id).await?,
    }))
}

/// The form a document already pinned.
///
/// **Falls back to the type's current binding when the document pinned
/// nothing**, which is the only situation `guard_rebinding` refuses over and the
/// only one where a document has nothing of its own to render against. Such a
/// document cannot be created by [`super::document::create_document`]; it can
/// exist because a type bound no form when it was created, or because a row was
/// written by hand.
pub async fn pinned_form_of(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    locked: &LockedDocument,
) -> Result<PinnedForm, AppError> {
    if locked.form_id.is_some() {
        return Ok(PinnedForm {
            form_id: locked.form_id,
            definition: definition_of(transaction, tenant_id, locked.form_id).await?,
        });
    }

    Ok(
        lock_pinned_form(transaction, tenant_id, locked.document_type_id)
            .await?
            .unwrap_or(PinnedForm {
                form_id: None,
                definition: None,
            }),
    )
}

/// The JFSS definition behind a form id.
///
/// **Soft-deleted revisions are read too**, and that is deliberate rather than
/// an oversight in the predicate. A document pins a revision forever, and a form
/// retired years later must not make every document that pinned it unreadable —
/// which is the "orphaned into an unreadable state" failure #170 AC5 asks about,
/// arriving on the form side instead of the entity side. The revision is
/// immutable once published, so what is read is still exactly what was filled
/// in.
async fn definition_of(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    form_id: Option<Uuid>,
) -> Result<Option<serde_json::Value>, AppError> {
    let Some(form_id) = form_id else {
        return Ok(None);
    };

    let row = sqlx::query_scalar!(
        "SELECT definition_json FROM rad_forms WHERE tenant_id = $1 AND id = $2",
        tenant_id,
        form_id
    )
    .fetch_optional(&mut **transaction)
    .await?;

    Ok(row)
}
