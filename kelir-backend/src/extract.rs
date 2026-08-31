//! Request extractors that fail inside the error envelope.
//!
//! **Why this exists.** Axum's own extractors reject with their own plain-text
//! responses, which are neither the envelope every other error uses (coding
//! standard §2.6) nor something a form can act on: they name no field. Worse, a
//! body that deserializes *successfully* while dropping what the client sent is
//! not an error at all — a client posting `role_ids` for `roleIds` got 201
//! Created and a user with no roles (#62).
//!
//! [`JsonBody`] closes both halves for the body. `#[serde(deny_unknown_fields)]`
//! on the request structs turns the silent drop into a deserialization failure,
//! and this extractor turns that failure into a 422 naming the field the client
//! actually sent.
//!
//! [`QueryParams`] and [`PathParam`] close the same hole on the other two places
//! a request carries data. `?page=abc` used to be a bare 400 with an **empty
//! body** on every list endpoint in the product (#122), which is the one shape a
//! client written against the envelope cannot read: it finds `null` where it
//! expects `error.code`.
//!
//! [`MultipartBody`] is the fourth, and it arrived the way the first three did —
//! by the claim above being false. This module's header used to say *the three
//! extractors together mean no refusal leaves this API outside the envelope*,
//! and the first route to take a file
//! ([#244](https://github.com/sujanto-gaws/kelir/issues/244)) answered **400
//! with a null body** to a caller who posted JSON to it, because
//! `axum::extract::Multipart` rejects before any handler code runs. A fourth
//! place a request carries data needed a fourth extractor; the sentence is now
//! true of four.
//!
//! **One gap, deliberately left.** The generated OpenAPI document does not say
//! `additionalProperties: false`, because utoipa 5 accepts that attribute only
//! on map-like schemas, not on a named struct. So a generated client will still
//! *send* an unknown field happily; it just gets a 422 naming it instead of a
//! 201 hiding it. Closing that needs a change in utoipa or a hand-written
//! schema, and the schema is generated and never hand-edited (coding standard
//! §2.6).

use axum::body::Bytes;
use axum::extract::{FromRequest, FromRequestParts, Request};
use axum::http::header;
use axum::http::request::Parts;
use serde::de::DeserializeOwned;

use crate::error::{AppError, ValidationDetail};

/// A JSON request body, rejected through [`AppError`].
///
/// Drop-in for `axum::Json` in a handler argument position. The `utoipa`
/// annotation is unaffected: `request_body` names the type, not the extractor.
#[derive(Debug, Clone, Copy, Default)]
pub struct JsonBody<T>(pub T);

impl<T, S> FromRequest<S> for JsonBody<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request(request: Request, state: &S) -> Result<Self, Self::Rejection> {
        if !is_json(&request) {
            return Err(AppError::UnsupportedMediaType);
        }

        let bytes = Bytes::from_request(request, state)
            .await
            .map_err(|error| AppError::bad_request(error.body_text()))?;

        let deserializer = &mut serde_json::Deserializer::from_slice(&bytes);

        serde_path_to_error::deserialize(deserializer)
            .map(Self)
            .map_err(rejection)
    }
}

/// Mirrors `axum::Json`'s own check: `application/json`, or any `+json` suffix.
fn is_json(request: &Request) -> bool {
    let Some(value) = request.headers().get(header::CONTENT_TYPE) else {
        return false;
    };
    let Ok(value) = value.to_str() else {
        return false;
    };
    let essence = value.split(';').next().unwrap_or_default().trim();

    essence.eq_ignore_ascii_case("application/json")
        || essence
            .rsplit_once('+')
            .is_some_and(|(_, suffix)| suffix.eq_ignore_ascii_case("json"))
}

/// Query-string parameters, rejected through [`AppError`].
///
/// Drop-in for `axum::extract::Query`. The `utoipa` annotation is unaffected:
/// `params(...)` names the type, not the extractor.
///
/// **Why this is not a wrapper around `axum::extract::Query`.** Axum already
/// deserializes through `serde_path_to_error`, so it knows which parameter
/// failed — but it keeps that knowledge inside a `Display` string on a rejection
/// that renders as plain text. Recovering the name would mean parsing English
/// back out of `"Failed to deserialize query string: pageSize: invalid digit
/// found in string"`, which breaks the first time axum rewords it. Repeating the
/// four lines axum's own `try_from_uri` runs costs less and hands the parameter
/// name over as data.
#[derive(Debug, Clone, Copy, Default)]
pub struct QueryParams<T>(pub T);

impl<T, S> FromRequestParts<S> for QueryParams<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let query = parts.uri.query().unwrap_or_default();
        let deserializer =
            serde_urlencoded::Deserializer::new(form_urlencoded::parse(query.as_bytes()));

        serde_path_to_error::deserialize(deserializer)
            .map(Self)
            .map_err(query_rejection)
    }
}

/// A path parameter, rejected through [`AppError`].
///
/// Drop-in for `axum::extract::Path`. Unlike [`QueryParams`] this *does* wrap the
/// axum extractor: `Path` reads the captures the router put in the request
/// extensions, which is routing state rather than anything re-derivable from the
/// URI, and its rejection already carries the parameter name as structured data
/// (`ErrorKind`) rather than only in a message.
///
/// The status codes axum chose are kept. A path segment that will not parse is
/// still a 400 — the resource named by `/parties/not-a-uuid` does not fail to
/// exist, the reference to it never became one — and the two programmer-error
/// kinds are still 500. All that changes is that the reply is now the envelope.
#[derive(Debug, Clone, Copy, Default)]
pub struct PathParam<T>(pub T);

impl<T, S> FromRequestParts<S> for PathParam<T>
where
    T: DeserializeOwned + Send,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        axum::extract::Path::<T>::from_request_parts(parts, state)
            .await
            .map(|axum::extract::Path(value)| Self(value))
            .map_err(path_rejection)
    }
}

/// Turns a query-string deserialization failure into a 422 naming the parameter.
///
/// 422 rather than 400, and a [`ValidationDetail`] rather than a message,
/// because `?page=abc` is the same class of mistake as `?statusId=NOPE`, which
/// `RoleViewQuery::filters` has always answered that way. One struct answering
/// two shapes for two of its own parameters is what #122 was filed about.
///
/// The path `serde_path_to_error` reports is the key as it appeared on the wire,
/// so a caller who sent `pageSize` is told `pageSize` even though the field is
/// `page_size`.
/// A `multipart/form-data` body that fails inside the envelope.
///
/// **The rejection happens before the handler**, which is what made this
/// necessary: `Multipart` refuses a body whose content type is not
/// `multipart/form-data` with a boundary, and refuses it with axum's own reply
/// rather than with this API's. A client written against the envelope finds
/// `null` where it expects `error.code`, which is exactly [#122]'s shape one
/// content type over.
///
/// **415 and not 422**, for [`AppError::UnsupportedMediaType`]'s stated reason:
/// the fix is a header rather than a payload. A caller who posted JSON here has
/// not sent a bad file, they have sent the wrong kind of request.
///
/// [#122]: https://github.com/sujanto-gaws/kelir/issues/122
pub struct MultipartBody(pub axum::extract::Multipart);

impl<S> FromRequest<S> for MultipartBody
where
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request(request: Request, state: &S) -> Result<Self, Self::Rejection> {
        axum::extract::Multipart::from_request(request, state)
            .await
            .map(Self)
            .map_err(|rejection| {
                tracing::debug!(%rejection, "a multipart body was refused before the handler");

                AppError::UnsupportedMediaType
            })
    }
}

fn query_rejection(error: serde_path_to_error::Error<serde_urlencoded::de::Error>) -> AppError {
    let path = error.path().to_string();
    let parameter = if path.is_empty() || path == "." {
        "query".to_owned()
    } else {
        path
    };

    AppError::validation(vec![ValidationDetail::new(
        parameter,
        "type",
        "INVALID_TYPE",
        capitalize(&error.inner().to_string()),
    )])
}

/// Turns a path-parameter rejection into the envelope, keeping axum's status.
///
/// The status codes are the ones axum already chose, so this changes the shape
/// of the reply and not the contract. A path segment that will not parse stays a
/// 400: `/parties/not-a-uuid` does not name a resource that is missing, it fails
/// to name one at all. The two kinds axum answers 500 to describe a handler
/// signature that does not match its route — a defect here, not in the request —
/// and stay 500, saying nothing to the caller like every other internal error.
fn path_rejection(rejection: axum::extract::rejection::PathRejection) -> AppError {
    use axum::extract::path::ErrorKind;
    use axum::extract::rejection::PathRejection;

    let PathRejection::FailedToDeserializePathParams(failure) = rejection else {
        // `MissingPathParams`: the router captured nothing for a handler that
        // asked for a capture. Wiring, not input.
        return AppError::Internal {
            source: anyhow::anyhow!("path parameters missing from the request extensions"),
        };
    };

    match failure.into_kind() {
        ErrorKind::ParseErrorAtKey {
            key, expected_type, ..
        } => AppError::bad_request(format!(
            "`{key}` in the path is not a valid {expected_type}"
        )),
        ErrorKind::DeserializeError { key, message, .. } => {
            AppError::bad_request(format!("`{key}` in the path is not valid: {message}"))
        }
        ErrorKind::InvalidUtf8InPathParam { key } => AppError::bad_request(format!(
            "`{key}` in the path is not valid UTF-8 once decoded"
        )),
        ErrorKind::ParseError { expected_type, .. }
        | ErrorKind::ParseErrorAtIndex { expected_type, .. } => {
            AppError::bad_request(format!("The path is not a valid {expected_type}"))
        }
        ErrorKind::Message(message) => AppError::bad_request(capitalize(&message)),
        kind @ (ErrorKind::UnsupportedType { .. } | ErrorKind::WrongNumberOfParameters { .. }) => {
            AppError::Internal {
                source: anyhow::anyhow!("path extractor does not match its route: {kind}"),
            }
        }
        // `ErrorKind` is `#[non_exhaustive]`. A kind added later is answered as
        // input rather than as an internal error: five of the seven that exist
        // today describe the request, and reporting a caller's mistake as a 500
        // is the worse of the two ways to be wrong about a new one.
        kind => AppError::bad_request(capitalize(&kind.to_string())),
    }
}

/// Turns a deserialization failure into the error a client can act on.
///
/// Malformed JSON is a 400: the request never became data, so there is no field
/// to name. Anything that parsed as JSON but did not fit the type is a 422 with
/// a [`ValidationDetail`], because that is a payload the caller can correct
/// field by field — the same shape JFSS-driven forms already render (JSON Form
/// Schema S10.3).
fn rejection(error: serde_path_to_error::Error<serde_json::Error>) -> AppError {
    use serde_json::error::Category;

    let path = error.path().to_string();
    let inner = error.inner();

    match inner.classify() {
        Category::Syntax | Category::Eof => AppError::bad_request("Request body is not valid JSON"),
        Category::Io => AppError::bad_request("Request body could not be read"),
        Category::Data => AppError::validation(vec![detail(&path, &strip_position(inner))]),
    }
}

/// Serde names the offending field only in its message; `serde_path_to_error`
/// gives the position. The two overlap differently per case, which is why this
/// is not one rule: an *unknown* field is already the tail of the reported path,
/// while a *missing* one cannot be — the path stops at the object that lacks it.
/// Both shapes are pinned by the tests below.
fn detail(path: &str, message: &str) -> ValidationDetail {
    if let Some(field) = quoted_field_after(message, "unknown field ") {
        return ValidationDetail::new(
            if path.is_empty() {
                field.clone()
            } else {
                path.to_owned()
            },
            "unknownField",
            "UNKNOWN_FIELD",
            format!(
                "Unknown field `{field}`. Check the spelling and casing — the API is camelCase."
            ),
        );
    }

    if let Some(field) = quoted_field_after(message, "missing field ") {
        return ValidationDetail::new(
            join(path, &field),
            "required",
            "REQUIRED",
            format!("`{field}` is required"),
        );
    }

    // Everything else is a value that parsed but did not fit: wrong type, a
    // string where a UUID belongs, an enum variant that does not exist. Here
    // the path is exact, which is why it is worth carrying.
    ValidationDetail::new(
        if path.is_empty() { "body" } else { path },
        "type",
        "INVALID_TYPE",
        capitalize(message),
    )
}

/// Extracts `x` from a message reading ``<prefix>`x`, …``.
fn quoted_field_after(message: &str, prefix: &str) -> Option<String> {
    let rest = message.strip_prefix(prefix)?;
    let rest = rest.strip_prefix('`')?;
    let (field, _) = rest.split_once('`')?;

    Some(field.to_owned())
}

/// `serde_path_to_error` reports `.` for the root; a field there is just the
/// field, and one nested inside `items[0]` is `items[0].field`.
fn join(path: &str, field: &str) -> String {
    if path.is_empty() || path == "." {
        field.to_owned()
    } else {
        format!("{path}.{field}")
    }
}

/// Drops serde's ` at line 1 column 42` suffix. Byte offsets describe the wire
/// format, and the client is correcting a field.
fn strip_position(error: &serde_json::Error) -> String {
    let message = error.to_string();

    match message.find(" at line ") {
        Some(index) => message[..index].to_owned(),
        None => message,
    }
}

fn capitalize(message: &str) -> String {
    let mut characters = message.chars();

    match characters.next() {
        Some(first) => first.to_uppercase().collect::<String>() + characters.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use axum::routing::post;
    use axum::Router;
    use serde::Deserialize;
    use tower::ServiceExt;

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Payload {
        name: String,
        count: u32,
        #[serde(default)]
        tags: Vec<Tag>,
    }

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Tag {
        label: String,
    }

    /// Echoes every field back, so a test can assert the values arrived and not
    /// merely that the status was 200.
    async fn accept(JsonBody(payload): JsonBody<Payload>) -> String {
        let labels: Vec<&str> = payload.tags.iter().map(|tag| tag.label.as_str()).collect();

        format!("{}:{}:{}", payload.name, payload.count, labels.join(","))
    }

    /// Posts a body and returns what the client would see.
    async fn post_raw(content_type: &str, body: &str) -> (StatusCode, Vec<u8>) {
        let response = Router::new()
            .route("/", post(accept))
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/")
                    .header("content-type", content_type)
                    .body(Body::from(body.to_owned()))
                    .expect("request builds"),
            )
            .await
            .expect("router responds");

        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body reads");

        (status, bytes.to_vec())
    }

    async fn post_body(content_type: &str, body: &str) -> (StatusCode, serde_json::Value) {
        let (status, bytes) = post_raw(content_type, body).await;

        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
        )
    }

    async fn post_json(body: &str) -> (StatusCode, serde_json::Value) {
        post_body("application/json", body).await
    }

    /// The success path answers plain text, not the envelope.
    async fn post_text(body: &str) -> (StatusCode, String) {
        let (status, bytes) = post_raw("application/json", body).await;

        (status, String::from_utf8(bytes).expect("body is utf-8"))
    }

    /// The first detail of an error envelope.
    fn detail(body: &serde_json::Value) -> &serde_json::Value {
        &body["error"]["details"][0]
    }

    #[tokio::test]
    async fn accepts_a_well_formed_body() {
        let (status, echoed) = post_text(r#"{"name":"a","count":1,"tags":[{"label":"x"}]}"#).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(echoed, "a:1:x", "every field should survive extraction");
    }

    #[tokio::test]
    async fn rejects_an_unknown_field_by_name() {
        // #62: this body previously succeeded, silently discarding `Count` and
        // defaulting the field it was meant to set.
        let (status, body) = post_json(r#"{"name":"a","count":1,"Count":2}"#).await;

        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(body["error"]["code"], "VALIDATION_ERROR");
        assert_eq!(detail(&body)["path"], "Count");
        assert_eq!(detail(&body)["code"], "UNKNOWN_FIELD");
    }

    #[tokio::test]
    async fn names_the_field_the_client_sent_not_the_one_it_meant() {
        // The whole value of the message: `role_ids` is what the developer
        // typed, and `roleIds` is what they wanted. Echoing their spelling is
        // what makes the mistake findable.
        let (_, body) = post_json(r#"{"name":"a","count":1,"tag_s":[]}"#).await;

        assert_eq!(detail(&body)["path"], "tag_s");
        assert!(
            detail(&body)["message"]
                .as_str()
                .expect("message is a string")
                .contains("tag_s"),
            "message should quote the offending field: {body}"
        );
    }

    #[tokio::test]
    async fn rejects_a_missing_required_field() {
        let (status, body) = post_json(r#"{"name":"a"}"#).await;

        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(detail(&body)["path"], "count");
        assert_eq!(detail(&body)["code"], "REQUIRED");
        assert_eq!(detail(&body)["rule"], "required");
    }

    #[tokio::test]
    async fn rejects_a_wrong_type_and_names_the_field() {
        let (status, body) = post_json(r#"{"name":"a","count":"many"}"#).await;

        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(detail(&body)["path"], "count");
        assert_eq!(detail(&body)["code"], "INVALID_TYPE");
    }

    #[tokio::test]
    async fn reports_the_full_path_of_a_nested_field() {
        // Why `serde_path_to_error` is a dependency: serde_json alone reports
        // "invalid type" with no idea which element it was in.
        let (_, body) =
            post_json(r#"{"name":"a","count":1,"tags":[{"label":"x"},{"label":2}]}"#).await;

        assert_eq!(detail(&body)["path"], "tags[1].label");
    }

    #[tokio::test]
    async fn reports_an_unknown_nested_field_under_its_parent() {
        let (_, body) =
            post_json(r#"{"name":"a","count":1,"tags":[{"label":"x","colour":"red"}]}"#).await;

        assert_eq!(detail(&body)["path"], "tags[0].colour");
        assert_eq!(detail(&body)["code"], "UNKNOWN_FIELD");
    }

    #[tokio::test]
    async fn malformed_json_is_a_bad_request_not_a_validation_failure() {
        // Nothing became data, so there is no field to name — and a 422 with an
        // empty details array would be a worse answer than a 400.
        let (status, body) = post_json(r#"{"name":"a",,}"#).await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "BAD_REQUEST");
    }

    #[tokio::test]
    async fn an_empty_body_is_a_bad_request() {
        let (status, _) = post_json("").await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn a_non_json_content_type_is_refused() {
        let (status, body) = post_body("text/plain", r#"{"name":"a","count":1}"#).await;

        assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
        assert_eq!(body["error"]["code"], "UNSUPPORTED_MEDIA_TYPE");
    }

    #[tokio::test]
    async fn a_json_suffix_content_type_is_accepted() {
        // `application/merge-patch+json` and friends are JSON; refusing them
        // would be a lie about what the endpoint can read.
        let (status, _) =
            post_body("application/merge-patch+json", r#"{"name":"a","count":1}"#).await;

        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn a_charset_parameter_does_not_defeat_the_content_type_check() {
        let (status, _) = post_body(
            "application/json; charset=utf-8",
            r#"{"name":"a","count":1}"#,
        )
        .await;

        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn the_error_message_carries_no_byte_offsets() {
        // "at line 1 column 42" describes the wire format; the client is
        // correcting a field, and the path already says which.
        let (_, body) = post_json(r#"{"name":"a","count":"many"}"#).await;

        let message = detail(&body)["message"]
            .as_str()
            .expect("message is a string");

        assert!(
            !message.contains("at line"),
            "message should not carry byte offsets: {message}"
        );
    }

    // -----------------------------------------------------------------------
    // QueryParams (#122)
    // -----------------------------------------------------------------------

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Filters {
        page: Option<u32>,
        page_size: Option<u32>,
        search: Option<String>,
    }

    async fn filter(QueryParams(filters): QueryParams<Filters>) -> String {
        format!(
            "{}:{}:{}",
            filters.page.unwrap_or_default(),
            filters.page_size.unwrap_or_default(),
            filters.search.unwrap_or_default()
        )
    }

    async fn get_query(query: &str) -> (StatusCode, serde_json::Value) {
        let response = Router::new()
            .route("/", axum::routing::get(filter))
            .oneshot(
                Request::builder()
                    .uri(format!("/?{query}"))
                    .body(Body::empty())
                    .expect("request builds"),
            )
            .await
            .expect("router responds");

        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body reads");

        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
        )
    }

    #[tokio::test]
    async fn accepts_a_well_formed_query_string() {
        let (status, _) = get_query("page=2&pageSize=50&search=acme").await;

        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn a_non_numeric_page_is_a_422_inside_the_envelope() {
        let (status, body) = get_query("page=abc").await;

        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(body["success"], false);
        assert_eq!(body["error"]["code"], "VALIDATION_ERROR");
    }

    #[tokio::test]
    async fn a_negative_page_is_a_422_inside_the_envelope() {
        // The parameter is `u32`, so `-1` fails to parse rather than arriving
        // and being clamped. It is a refusal either way; #122 is about where.
        let (status, body) = get_query("page=-1").await;

        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(body["error"]["code"], "VALIDATION_ERROR");
    }

    #[tokio::test]
    async fn a_bad_query_parameter_is_named_as_the_client_spelled_it() {
        // The field is `page_size`; the wire name is `pageSize`. A caller
        // correcting their request needs the second.
        let (_, body) = get_query("pageSize=x").await;

        assert_eq!(detail(&body)["path"], "pageSize");
        assert_eq!(detail(&body)["code"], "INVALID_TYPE");
    }

    #[tokio::test]
    async fn a_bad_query_parameter_carries_a_body_at_all() {
        // The defect #122 records is not the status but the empty body: a
        // client written against the envelope finds `null` where `error.code`
        // should be. This is the assertion that goes red if the extractor is
        // swapped back for `axum::extract::Query`.
        let (_, body) = get_query("page=abc").await;

        assert!(
            body["error"]["code"].is_string(),
            "the refusal should be readable as the error envelope, got {body}"
        );
    }

    #[tokio::test]
    async fn an_unknown_query_parameter_is_ignored_rather_than_refused() {
        // Deliberate, and unchanged by #122: `Pagination` has always ignored
        // what it does not recognise, and `deny_unknown_fields` on one query
        // struct and not the others would be a difference with no reason behind
        // it (coding standard §1.1).
        let (status, _) = get_query("page=1&somethingElse=1").await;

        assert_eq!(status, StatusCode::OK);
    }

    // -----------------------------------------------------------------------
    // PathParam (#122)
    // -----------------------------------------------------------------------

    async fn identify(PathParam(id): PathParam<uuid::Uuid>) -> String {
        id.to_string()
    }

    async fn get_path(path: &str) -> (StatusCode, serde_json::Value) {
        let response = Router::new()
            .route("/{id}", axum::routing::get(identify))
            .oneshot(
                Request::builder()
                    .uri(format!("/{path}"))
                    .body(Body::empty())
                    .expect("request builds"),
            )
            .await
            .expect("router responds");

        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body reads");

        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
        )
    }

    #[tokio::test]
    async fn accepts_a_well_formed_path_parameter() {
        let (status, _) = get_path("0198f0e2-0000-7000-8000-000000000000").await;

        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn an_unparseable_path_parameter_stays_a_400_and_joins_the_envelope() {
        let (status, body) = get_path("not-a-uuid").await;

        // The status is the one axum already answered. What changes is that
        // there is now a body to read.
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "BAD_REQUEST");
        assert!(
            body["error"]["message"]
                .as_str()
                .is_some_and(|message| message.contains("id")),
            "the message should name the parameter, got {body}"
        );
    }
}
