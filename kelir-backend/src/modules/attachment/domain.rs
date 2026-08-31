//! What an attachment is, and the refusals that need no database (FR-ATT-001,
//! FR-ATT-003; [#244]).
//!
//! [#244]: https://github.com/sujanto-gaws/kelir/issues/244

use chrono::{DateTime, Utc};
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::error::{AppError, ValidationDetail};

/// How far an attachment has got through the scanner (Database Schema §8.2).
///
/// **Every variant but `Clean` is a refusal to serve the bytes**, and that rule
/// belongs to [#246](https://github.com/sujanto-gaws/kelir/issues/246) rather
/// than here — this item writes `Pending` and nothing else. The enum carries all
/// four because the column does, and a type that could not represent a row the
/// database permits would be a type that panics on a row somebody else wrote.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum VirusScanStatus {
    Pending,
    Clean,
    Infected,
    Failed,
}

impl VirusScanStatus {
    pub fn as_db(self) -> &'static str {
        match self {
            Self::Pending => "PENDING",
            Self::Clean => "CLEAN",
            Self::Infected => "INFECTED",
            Self::Failed => "FAILED",
        }
    }

    /// **Unknown reads as `Failed`, not as `Clean`.** A value this binary does
    /// not know is a value a later release wrote, and the only safe reading of
    /// *I do not know what happened to this file* is that it has not cleared —
    /// which is the same direction #246 AC2 takes for a scan that could not run.
    pub fn from_db(value: &str) -> Self {
        match value {
            "PENDING" => Self::Pending,
            "CLEAN" => Self::Clean,
            "INFECTED" => Self::Infected,
            _ => Self::Failed,
        }
    }
}

/// One attachment, as the API reports it.
///
/// **No `storage_reference`.** Where the bytes are is this process's business:
/// a caller who knows the object path knows the shape of the bucket, and the
/// only thing they can do with it is guess at another one. The id is what a
/// caller needs, and the download route takes the id.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    pub id: Uuid,
    pub document_id: Uuid,
    /// The name as uploaded, which is the one the person recognises.
    pub original_file_name: String,
    pub mime_type: String,
    pub file_size: i64,
    /// `sha256:...` over the bytes as stored.
    pub checksum: String,
    pub description: Option<String>,
    pub virus_scan_status: VirusScanStatus,
    pub created_at: DateTime<Utc>,
    pub created_by: Option<Uuid>,
}

/// Refuses a file larger than the deployment accepts ([#245] AC3, AC6).
///
/// **Named limit, named unit.** *Upload failed* sends a person back to try the
/// same file again, which is #245 AC6's own reasoning; a number they can compare
/// against the file they are holding does not.
///
/// [#245]: https://github.com/sujanto-gaws/kelir/issues/245
pub fn file_too_large(limit: usize) -> AppError {
    AppError::validation(vec![ValidationDetail::new(
        "file",
        "maxSize",
        "FILE_TOO_LARGE",
        format!(
            "this deployment accepts files up to {limit} bytes ({} MB)",
            limit / (1024 * 1024)
        ),
    )])
}

/// Refuses a file whose **content** is not a type this deployment stores
/// ([#245] AC4, AC6).
///
/// The message names what the bytes turned out to be rather than what the caller
/// called the file, because those are the two different facts and the second one
/// is the one they already believe.
pub fn type_not_allowed(detected: Option<&str>, allowed: &[String]) -> AppError {
    let what = detected.unwrap_or("not a type this server recognises");

    AppError::validation(vec![ValidationDetail::new(
        "file",
        "mimeType",
        "FILE_TYPE_NOT_ALLOWED",
        format!(
            "this file's content is {what}; this deployment stores {}",
            allowed.join(", ")
        ),
    )])
}

/// What the bytes actually are, by their leading bytes.
///
/// **Never the extension, and never the caller's `Content-Type`** ([#245] AC4).
/// Both are text the caller wrote. `mime_type` is still *recorded* from what
/// they declared, because it is a fact about the request worth keeping; what it
/// is not is evidence.
///
/// `None` means *nothing recognised it*, which is a refusal rather than a pass:
/// an allow-list whose unknown case is "allow" is not an allow-list.
pub fn detect_mime_type(bytes: &[u8]) -> Option<&'static str> {
    infer::get(bytes).map(|kind| kind.mime_type())
}

/// Whether the detected type is one this deployment stores.
///
/// **An empty allow-list refuses everything.** A deployment that configures the
/// list to nothing has said *store nothing*, and reading that as *store
/// anything* would make the one obvious misconfiguration the dangerous one.
pub fn type_is_allowed(detected: Option<&str>, allowed: &[String]) -> bool {
    let Some(detected) = detected else {
        return false;
    };

    allowed
        .iter()
        .any(|entry| entry.eq_ignore_ascii_case(detected))
}

/// Refuses the bytes of an attachment the scanner has not cleared
/// ([#246](https://github.com/sujanto-gaws/kelir/issues/246) AC2, AC3).
///
/// **Three states, three messages.** *Not yet* and *never* need different things
/// from the person holding the file: one is waiting, the other is a file they
/// should replace. `FAILED` is a refusal and not a pass — a scan that could not
/// run has cleared nothing — and it says so rather than reading as an error in
/// the download.
///
/// A **409** rather than a 403 or a 404: the attachment is there and this caller
/// may read it, but its state is not one the bytes can be served from. That is a
/// conflict with the resource's condition, which is what a person retrying in a
/// minute needs to be told.
pub fn not_yet_cleared(status: VirusScanStatus) -> AppError {
    AppError::conflict(match status {
        VirusScanStatus::Pending => {
            "this file has not been scanned yet, so it cannot be downloaded. Try again shortly"
                .to_owned()
        }
        VirusScanStatus::Infected => {
            "this file was found to be infected and will not be served. Remove it and upload a \
             clean copy"
                .to_owned()
        }
        VirusScanStatus::Failed => {
            "this file could not be scanned, so it will not be served — a scan that did not run \
             has cleared nothing. Upload it again"
                .to_owned()
        }
        // Unreachable: the caller checks for `Clean` before calling this.
        VirusScanStatus::Clean => "this file is available".to_owned(),
    })
}

/// The longest `original_file_name` this API will record.
///
/// The column is `TEXT` and takes anything; this is the bound that stops a
/// megabyte of filename being stored because nothing said otherwise. It is the
/// same instinct as `normalize_comment`'s one module over, and the same shape of
/// refusal: named field, stated limit.
pub const MAX_FILE_NAME: usize = 255;

/// A file part that is missing, empty, or named something unusable.
///
/// **Three refusals rather than one**, because "upload failed" sends a person
/// back to try the same file again — which is the reasoning
/// [#245](https://github.com/sujanto-gaws/kelir/issues/245) AC6 states for the
/// two refusals that item adds, applied here first.
pub fn no_file_part() -> AppError {
    AppError::validation(vec![ValidationDetail::new(
        "file",
        "required",
        "FILE_REQUIRED",
        "attach the file in a multipart field named `file`",
    )])
}

pub fn empty_file() -> AppError {
    AppError::validation(vec![ValidationDetail::new(
        "file",
        "empty",
        "FILE_EMPTY",
        "the file has no content; an empty attachment says nothing about the document",
    )])
}

pub fn file_name_too_long(length: usize) -> AppError {
    AppError::validation(vec![ValidationDetail::new(
        "file",
        "maxLength",
        "FILE_NAME_TOO_LONG",
        format!("the file name is {length} characters; the limit is {MAX_FILE_NAME}"),
    )])
}

/// The name to store the file under, which is never the name it arrived with.
///
/// **`file_name` is derived and `original_file_name` is kept**, which is why
/// §8.2 has both. An uploaded name is caller-controlled text that has been a
/// path traversal, a shell argument and a Windows reserved device in various
/// products; the stored name is this function's output, and the original is
/// carried alongside it as *data* so a person still sees what they uploaded.
///
/// **The basename first**, and that is the step this function was written
/// without. A browser sends what the user picked, and on some platforms that is
/// a path; `../../etc/passwd` mapped character by character becomes
/// `_.._.._etc_passwd`, which is safe and reads like something that got away.
/// Taking everything after the last `/` or `\\` means the stored name is the
/// file's name rather than a flattened rendering of where it came from — and it
/// makes the property checkable in one line instead of argued about.
///
/// Then: everything that is not an ASCII letter, digit, dot, dash or underscore
/// becomes an underscore, leading dots are dropped so nothing becomes hidden or
/// relative, and an empty result becomes `file`.
pub fn safe_file_name(original: &str) -> String {
    let basename = original.rsplit(['/', '\\']).next().unwrap_or(original);

    let mapped: String = basename
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect();

    let trimmed = mapped.trim_start_matches('.').to_owned();

    if trimmed.is_empty() {
        "file".to_owned()
    } else {
        trimmed
    }
}

/// Where the object goes, and it is generated here rather than taken from the
/// request ([#244] AC6).
///
/// **A caller-supplied path is a caller-chosen destination**, which is an
/// overwrite of somebody else's object one guess away. The tenant and the
/// document scope it, the attachment's own id makes it unique, and the safe name
/// is on the end only so that a person looking in the bucket can see what a
/// thing is.
pub fn storage_reference(
    tenant_id: Uuid,
    document_id: Uuid,
    attachment_id: Uuid,
    file_name: &str,
) -> String {
    format!("tenants/{tenant_id}/documents/{document_id}/attachments/{attachment_id}/{file_name}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_traversal_in_an_uploaded_name_does_not_survive_into_the_stored_one() {
        // The basename, not a flattened path: what is stored is the file's name.
        assert_eq!(safe_file_name("../../etc/passwd"), "passwd");
        assert_eq!(safe_file_name("..\\..\\windows\\system32"), "system32");
        // And no `..` reaches the object key from either separator.
        assert!(!safe_file_name("../../etc/passwd").contains(".."));
    }

    #[test]
    fn a_path_whose_last_segment_is_a_dot_segment_still_produces_something_storable() {
        assert_eq!(safe_file_name("evidence/.."), "file");
        assert_eq!(safe_file_name("/"), "file");
    }

    #[test]
    fn a_name_that_is_only_punctuation_still_produces_something_storable() {
        assert_eq!(safe_file_name("..."), "file");
        assert_eq!(safe_file_name(""), "file");
    }

    #[test]
    fn an_ordinary_name_is_left_alone() {
        assert_eq!(safe_file_name("quotation-2026.pdf"), "quotation-2026.pdf");
    }

    #[test]
    fn a_storage_reference_names_the_tenant_the_document_and_the_attachment() {
        let tenant = Uuid::nil();
        let document = Uuid::nil();
        let attachment = Uuid::nil();
        let reference = storage_reference(tenant, document, attachment, "q.pdf");

        assert!(reference.starts_with(&format!("tenants/{tenant}/documents/{document}/")));
        assert!(reference.ends_with("/q.pdf"));
    }

    #[test]
    fn a_scan_status_this_binary_does_not_know_reads_as_failed_rather_than_clean() {
        assert_eq!(
            VirusScanStatus::from_db("QUARANTINED"),
            VirusScanStatus::Failed
        );
        assert_eq!(VirusScanStatus::from_db("CLEAN"), VirusScanStatus::Clean);
    }

    #[test]
    fn an_unrecognised_payload_is_refused_rather_than_allowed_through() {
        // An allow-list whose unknown case is "allow" is not an allow-list.
        assert!(!type_is_allowed(None, &["application/pdf".to_owned()]));
        assert!(!type_is_allowed(
            detect_mime_type(b"just some text, which no magic number claims"),
            &["application/pdf".to_owned()]
        ));
    }

    #[test]
    fn an_empty_allow_list_stores_nothing() {
        assert!(!type_is_allowed(Some("application/pdf"), &[]));
    }

    #[test]
    fn content_decides_the_type_and_the_name_does_not() {
        // A PDF header, whatever the file is called.
        let pdf = b"%PDF-1.7\n1 0 obj\n<<>>\nendobj\n";

        assert_eq!(detect_mime_type(pdf), Some("application/pdf"));
        assert!(type_is_allowed(
            detect_mime_type(pdf),
            &["application/pdf".to_owned()]
        ));
        // The same bytes are still refused where the deployment does not store
        // that type — the allow-list is the policy, the sniffer is the fact.
        assert!(!type_is_allowed(
            detect_mime_type(pdf),
            &["image/png".to_owned()]
        ));
    }
}
