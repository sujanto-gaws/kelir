//! Routes under `/api/v1/integration/external-systems` (FR-INT-001, #520) and
//! `/api/v1/integration/logs` (FR-INT-006, #548).
//!
//! **No `DELETE` on a system** — `POST {id}/deactivate` instead (#520, the
//! product owner's answer 2), and `POST {id}/activate` to undo it, both under
//! `:deactivate`. **No `DELETE` on an endpoint** either: an
//! endpoint is retired by a `PUT` setting `status` to `INACTIVE`, under the
//! system's `:update`. **A credential reference can be deleted**, under its own
//! `integration:credential:delete`; what it pointed at is untouched.

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use uuid::Uuid;

use super::domain::{
    CreateIntegrationCredentialRequest, CreateIntegrationEndpointRequest, ExternalSystem,
    ExternalSystemQuery, IntegrationCredential, IntegrationEndpoint, IntegrationLog,
    IntegrationLogQuery, IntegrationLogSummary, RegisterExternalSystemRequest, TestCallResponse,
    UpdateExternalSystemRequest, UpdateIntegrationCredentialRequest,
    UpdateIntegrationEndpointRequest,
};
use super::service::{credential, endpoint, external_system, log, test_call};
use crate::error::AppError;
use crate::extract::{JsonBody, PathParam, QueryParams};
use crate::middleware::auth::Authenticated;
use crate::response::{ItemEnvelope, ListEnvelope, Pagination};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/external-systems",
            get(list_external_systems).post(register_external_system),
        )
        .route(
            "/external-systems/{id}",
            get(get_external_system).put(update_external_system),
        )
        .route(
            "/external-systems/{id}/deactivate",
            post(deactivate_external_system),
        )
        .route(
            "/external-systems/{id}/activate",
            post(activate_external_system),
        )
        .route(
            "/external-systems/{id}/endpoints",
            get(list_endpoints).post(create_endpoint),
        )
        .route(
            "/external-systems/{id}/endpoints/{endpointId}",
            get(get_endpoint).put(update_endpoint),
        )
        .route(
            "/external-systems/{id}/endpoints/{endpointId}/test-call",
            post(test_call_endpoint),
        )
        .route(
            "/external-systems/{id}/credentials",
            get(list_credentials).post(create_credential),
        )
        .route(
            "/external-systems/{id}/credentials/{credentialId}",
            get(get_credential)
                .put(update_credential)
                .delete(delete_credential),
        )
        .route("/logs", get(list_logs))
        .route("/logs/{id}", get(get_log))
}

// ---------------------------------------------------------------------------
// External systems
// ---------------------------------------------------------------------------

#[utoipa::path(
    get, path = "/api/v1/integration/external-systems", tag = "integration",
    params(ExternalSystemQuery),
    responses(
        (status = 200, description = "One page of the tenant's external systems, ordered by systemCode", body = [ExternalSystem]),
        (status = 403, description = "Missing integration:external-system:read"),
        (status = 422, description = "A query parameter would not parse")
    ),
    security(("bearer" = []))
)]
pub async fn list_external_systems(
    State(state): State<AppState>,
    caller: Authenticated,
    QueryParams(query): QueryParams<ExternalSystemQuery>,
) -> Result<Json<ListEnvelope<ExternalSystem>>, AppError> {
    let (systems, meta) = external_system::list_external_systems(&state, &caller, &query).await?;

    Ok(Json(ListEnvelope::new(systems, meta)))
}

#[utoipa::path(
    get, path = "/api/v1/integration/external-systems/{id}", tag = "integration",
    responses(
        (status = 200, description = "The external system. Its credentials are not in it: they are read under integration:credential:read", body = ExternalSystem),
        (status = 403, description = "Missing integration:external-system:read"),
        (status = 404, description = "No such external system in this tenant")
    ),
    security(("bearer" = []))
)]
pub async fn get_external_system(
    State(state): State<AppState>,
    caller: Authenticated,
    PathParam(id): PathParam<Uuid>,
) -> Result<Json<ItemEnvelope<ExternalSystem>>, AppError> {
    Ok(Json(ItemEnvelope::new(
        external_system::get_external_system(&state, &caller, id).await?,
    )))
}

/// Register an external system. It is registered `ACTIVE`.
#[utoipa::path(
    post, path = "/api/v1/integration/external-systems", tag = "integration",
    request_body = RegisterExternalSystemRequest,
    responses(
        (status = 201, description = "Registered", body = ExternalSystem),
        (status = 403, description = "Missing integration:external-system:create"),
        (status = 409, description = "That systemCode is already in use in this tenant"),
        (status = 422, description = "Validation failed — including a baseUrl carrying a user name or password (CREDENTIALS_IN_URL) or a query string (QUERY_IN_BASE_URL)")
    ),
    security(("bearer" = []))
)]
pub async fn register_external_system(
    State(state): State<AppState>,
    caller: Authenticated,
    JsonBody(request): JsonBody<RegisterExternalSystemRequest>,
) -> Result<(StatusCode, Json<ItemEnvelope<ExternalSystem>>), AppError> {
    let system = external_system::register_external_system(&state, &caller, request).await?;

    Ok((StatusCode::CREATED, Json(ItemEnvelope::new(system))))
}

/// Edit an external system.
///
/// `systemCode` may not change. `status` may move between `ACTIVE` and
/// `MAINTENANCE`; `INACTIVE` is refused with `NOT_ALLOWED`, and so is any
/// `status` on a system that is `INACTIVE` — turning a system off and on is
/// `POST {id}/deactivate` and `POST {id}/activate`, under their own permission.
#[utoipa::path(
    put, path = "/api/v1/integration/external-systems/{id}", tag = "integration",
    request_body = UpdateExternalSystemRequest,
    responses(
        (status = 200, description = "Updated", body = ExternalSystem),
        (status = 403, description = "Missing integration:external-system:update"),
        (status = 404, description = "No such external system in this tenant"),
        (status = 422, description = "Validation failed — including a baseUrl carrying a user name or password (CREDENTIALS_IN_URL) or a query string (QUERY_IN_BASE_URL), and NOT_ALLOWED on status when the edit would move the system into or out of INACTIVE")
    ),
    security(("bearer" = []))
)]
pub async fn update_external_system(
    State(state): State<AppState>,
    caller: Authenticated,
    PathParam(id): PathParam<Uuid>,
    JsonBody(request): JsonBody<UpdateExternalSystemRequest>,
) -> Result<Json<ItemEnvelope<ExternalSystem>>, AppError> {
    Ok(Json(ItemEnvelope::new(
        external_system::update_external_system(&state, &caller, id, request).await?,
    )))
}

/// Deactivate an external system — `status` becomes `INACTIVE`.
///
/// Idempotent: a system already inactive is returned unchanged. Its endpoints
/// and credentials keep their own states. There is no delete.
#[utoipa::path(
    post, path = "/api/v1/integration/external-systems/{id}/deactivate", tag = "integration",
    responses(
        (status = 200, description = "Deactivated, or already inactive", body = ExternalSystem),
        (status = 403, description = "Missing integration:external-system:deactivate"),
        (status = 404, description = "No such external system in this tenant")
    ),
    security(("bearer" = []))
)]
pub async fn deactivate_external_system(
    State(state): State<AppState>,
    caller: Authenticated,
    PathParam(id): PathParam<Uuid>,
) -> Result<Json<ItemEnvelope<ExternalSystem>>, AppError> {
    Ok(Json(ItemEnvelope::new(
        external_system::deactivate_external_system(&state, &caller, id).await?,
    )))
}

/// Put an external system back in service — `status` becomes `ACTIVE`.
///
/// Under `integration:external-system:deactivate`: turning a system on and off
/// is one permission. Idempotent: a system already active is returned
/// unchanged.
#[utoipa::path(
    post, path = "/api/v1/integration/external-systems/{id}/activate", tag = "integration",
    responses(
        (status = 200, description = "Activated, or already active", body = ExternalSystem),
        (status = 403, description = "Missing integration:external-system:deactivate"),
        (status = 404, description = "No such external system in this tenant")
    ),
    security(("bearer" = []))
)]
pub async fn activate_external_system(
    State(state): State<AppState>,
    caller: Authenticated,
    PathParam(id): PathParam<Uuid>,
) -> Result<Json<ItemEnvelope<ExternalSystem>>, AppError> {
    Ok(Json(ItemEnvelope::new(
        external_system::activate_external_system(&state, &caller, id).await?,
    )))
}

// ---------------------------------------------------------------------------
// Endpoints — governed by the system's permissions
// ---------------------------------------------------------------------------

#[utoipa::path(
    get, path = "/api/v1/integration/external-systems/{id}/endpoints", tag = "integration",
    params(Pagination),
    responses(
        (status = 200, description = "One page of the system's endpoints, INACTIVE ones included, ordered by endpointCode", body = [IntegrationEndpoint]),
        (status = 403, description = "Missing integration:external-system:read"),
        (status = 404, description = "No such external system in this tenant")
    ),
    security(("bearer" = []))
)]
pub async fn list_endpoints(
    State(state): State<AppState>,
    caller: Authenticated,
    PathParam(id): PathParam<Uuid>,
    QueryParams(pagination): QueryParams<Pagination>,
) -> Result<Json<ListEnvelope<IntegrationEndpoint>>, AppError> {
    let (endpoints, meta) = endpoint::list_endpoints(&state, &caller, id, &pagination).await?;

    Ok(Json(ListEnvelope::new(endpoints, meta)))
}

#[utoipa::path(
    get, path = "/api/v1/integration/external-systems/{id}/endpoints/{endpointId}", tag = "integration",
    responses(
        (status = 200, description = "The endpoint", body = IntegrationEndpoint),
        (status = 403, description = "Missing integration:external-system:read"),
        (status = 404, description = "No such external system in this tenant, or no such endpoint on it")
    ),
    security(("bearer" = []))
)]
pub async fn get_endpoint(
    State(state): State<AppState>,
    caller: Authenticated,
    PathParam((id, endpoint_id)): PathParam<(Uuid, Uuid)>,
) -> Result<Json<ItemEnvelope<IntegrationEndpoint>>, AppError> {
    Ok(Json(ItemEnvelope::new(
        endpoint::get_endpoint(&state, &caller, id, endpoint_id).await?,
    )))
}

/// Add an endpoint to a system. It is created `ACTIVE`.
#[utoipa::path(
    post, path = "/api/v1/integration/external-systems/{id}/endpoints", tag = "integration",
    request_body = CreateIntegrationEndpointRequest,
    responses(
        (status = 201, description = "Created", body = IntegrationEndpoint),
        (status = 403, description = "Missing integration:external-system:update"),
        (status = 404, description = "No such external system in this tenant"),
        (status = 409, description = "The system already has an endpoint with that endpointCode"),
        (status = 422, description = "Validation failed")
    ),
    security(("bearer" = []))
)]
pub async fn create_endpoint(
    State(state): State<AppState>,
    caller: Authenticated,
    PathParam(id): PathParam<Uuid>,
    JsonBody(request): JsonBody<CreateIntegrationEndpointRequest>,
) -> Result<(StatusCode, Json<ItemEnvelope<IntegrationEndpoint>>), AppError> {
    let created = endpoint::create_endpoint(&state, &caller, id, request).await?;

    Ok((StatusCode::CREATED, Json(ItemEnvelope::new(created))))
}

/// Edit an endpoint. `status: INACTIVE` retires it; there is no delete.
#[utoipa::path(
    put, path = "/api/v1/integration/external-systems/{id}/endpoints/{endpointId}", tag = "integration",
    request_body = UpdateIntegrationEndpointRequest,
    responses(
        (status = 200, description = "Updated", body = IntegrationEndpoint),
        (status = 403, description = "Missing integration:external-system:update"),
        (status = 404, description = "No such external system in this tenant, or no such endpoint on it"),
        (status = 422, description = "Validation failed")
    ),
    security(("bearer" = []))
)]
pub async fn update_endpoint(
    State(state): State<AppState>,
    caller: Authenticated,
    PathParam((id, endpoint_id)): PathParam<(Uuid, Uuid)>,
    JsonBody(request): JsonBody<UpdateIntegrationEndpointRequest>,
) -> Result<Json<ItemEnvelope<IntegrationEndpoint>>, AppError> {
    Ok(Json(ItemEnvelope::new(
        endpoint::update_endpoint(&state, &caller, id, endpoint_id, request).await?,
    )))
}

/// Make a test call to an endpoint, and answer with what came back
/// (FR-INT-002, #547; ADR-0043).
///
/// **A real request**: the endpoint's `method` to the system's `baseUrl` joined
/// with the endpoint's `path`, with no body, carrying the system's one
/// credential that is active and valid today — `BEARER_TOKEN` as
/// `Authorization: Bearer`, `BASIC_AUTH` (`user:password`) as
/// `Authorization: Basic`. The request body is ignored; nothing in it can name
/// a URL, a host or a header.
///
/// The host is resolved once and every address it resolves to is checked:
/// loopback, link-local, unspecified and multicast are always refused, and a
/// private address unless `KELIR_INTEGRATION_ALLOWED_CIDRS` lists its range.
/// The connection is pinned to the checked address, redirects are not
/// followed, and the whole call is bounded by the system's `timeoutSeconds`.
///
/// **Every call from the moment the endpoint is found writes exactly one
/// `integration_logs` row**, answered or not; its id is `logId` here, and in
/// the message of a failure. No header is returned, and the body preview has
/// the secret redacted in every form it was sent in.
#[utoipa::path(
    post, path = "/api/v1/integration/external-systems/{id}/endpoints/{endpointId}/test-call", tag = "integration",
    responses(
        (status = 200, description = "The system answered. `status` is SUCCESS for a 2xx and FAILED for anything else, a 3xx included: redirects are not followed", body = TestCallResponse),
        (status = 403, description = "Missing integration:endpoint:call"),
        (status = 404, description = "No such external system in this tenant, or no such endpoint on it"),
        (status = 422, description = "The call was refused before anything was sent, and logged: EXTERNAL_SYSTEM_NOT_ACTIVE, ENDPOINT_NOT_ACTIVE, BASE_URL_MISSING, TARGET_URL_INVALID, NO_USABLE_CREDENTIAL, AMBIGUOUS_CREDENTIAL, CREDENTIAL_TYPE_NOT_SUPPORTED (only BEARER_TOKEN and BASIC_AUTH are built), SECRET_REFERENCE_MALFORMED, SECRET_NAME_NOT_PERMITTED (an env:// name outside the caller's tenant's KELIR_INTEGRATION_SECRET_<CODE>__*, where CODE is the tenant code upper case with - as _, or one another tenant's code also covers), SECRET_BACKEND_NOT_CONFIGURED (a vault:// reference), SECRET_NOT_FOUND, SECRET_MALFORMED, HOST_NOT_RESOLVED, EGRESS_REFUSED"),
        (status = 502, description = "UPSTREAM_UNREACHABLE — the connection failed or was closed; logged"),
        (status = 504, description = "UPSTREAM_TIMEOUT — no answer within the system's timeoutSeconds; logged")
    ),
    security(("bearer" = []))
)]
pub async fn test_call_endpoint(
    State(state): State<AppState>,
    caller: Authenticated,
    PathParam((id, endpoint_id)): PathParam<(Uuid, Uuid)>,
) -> Result<Json<ItemEnvelope<TestCallResponse>>, AppError> {
    Ok(Json(ItemEnvelope::new(
        test_call::test_call(&state, &caller, id, endpoint_id).await?,
    )))
}

// ---------------------------------------------------------------------------
// Credential references — under integration:credential:*
// ---------------------------------------------------------------------------

#[utoipa::path(
    get, path = "/api/v1/integration/external-systems/{id}/credentials", tag = "integration",
    params(Pagination),
    responses(
        (status = 200, description = "One page of the system's credential references, inactive ones included. Each is returned as it was stored; what was checked at write is its shape", body = [IntegrationCredential]),
        (status = 403, description = "Missing integration:credential:read"),
        (status = 404, description = "No such external system in this tenant")
    ),
    security(("bearer" = []))
)]
pub async fn list_credentials(
    State(state): State<AppState>,
    caller: Authenticated,
    PathParam(id): PathParam<Uuid>,
    QueryParams(pagination): QueryParams<Pagination>,
) -> Result<Json<ListEnvelope<IntegrationCredential>>, AppError> {
    let (credentials, meta) =
        credential::list_credentials(&state, &caller, id, &pagination).await?;

    Ok(Json(ListEnvelope::new(credentials, meta)))
}

#[utoipa::path(
    get, path = "/api/v1/integration/external-systems/{id}/credentials/{credentialId}", tag = "integration",
    responses(
        (status = 200, description = "The credential reference, as it was stored", body = IntegrationCredential),
        (status = 403, description = "Missing integration:credential:read"),
        (status = 404, description = "No such external system in this tenant, or no such credential on it")
    ),
    security(("bearer" = []))
)]
pub async fn get_credential(
    State(state): State<AppState>,
    caller: Authenticated,
    PathParam((id, credential_id)): PathParam<(Uuid, Uuid)>,
) -> Result<Json<ItemEnvelope<IntegrationCredential>>, AppError> {
    Ok(Json(ItemEnvelope::new(
        credential::get_credential(&state, &caller, id, credential_id).await?,
    )))
}

/// Add a credential reference: `vault://path[#field]` or `env://NAME`.
#[utoipa::path(
    post, path = "/api/v1/integration/external-systems/{id}/credentials", tag = "integration",
    request_body = CreateIntegrationCredentialRequest,
    responses(
        (status = 201, description = "Created", body = IntegrationCredential),
        (status = 403, description = "Missing integration:credential:create"),
        (status = 404, description = "No such external system in this tenant"),
        (status = 422, description = "Validation failed — NOT_A_SECRET_REFERENCE when secretReference is not a vault:// or env:// reference, WINDOW_INVERTED when validTo precedes validFrom")
    ),
    security(("bearer" = []))
)]
pub async fn create_credential(
    State(state): State<AppState>,
    caller: Authenticated,
    PathParam(id): PathParam<Uuid>,
    JsonBody(request): JsonBody<CreateIntegrationCredentialRequest>,
) -> Result<(StatusCode, Json<ItemEnvelope<IntegrationCredential>>), AppError> {
    let created = credential::create_credential(&state, &caller, id, request).await?;

    Ok((StatusCode::CREATED, Json(ItemEnvelope::new(created))))
}

#[utoipa::path(
    put, path = "/api/v1/integration/external-systems/{id}/credentials/{credentialId}", tag = "integration",
    request_body = UpdateIntegrationCredentialRequest,
    responses(
        (status = 200, description = "Updated", body = IntegrationCredential),
        (status = 403, description = "Missing integration:credential:update"),
        (status = 404, description = "No such external system in this tenant, or no such credential on it"),
        (status = 422, description = "Validation failed")
    ),
    security(("bearer" = []))
)]
pub async fn update_credential(
    State(state): State<AppState>,
    caller: Authenticated,
    PathParam((id, credential_id)): PathParam<(Uuid, Uuid)>,
    JsonBody(request): JsonBody<UpdateIntegrationCredentialRequest>,
) -> Result<Json<ItemEnvelope<IntegrationCredential>>, AppError> {
    Ok(Json(ItemEnvelope::new(
        credential::update_credential(&state, &caller, id, credential_id, request).await?,
    )))
}

#[utoipa::path(
    delete, path = "/api/v1/integration/external-systems/{id}/credentials/{credentialId}", tag = "integration",
    responses(
        (status = 204, description = "Removed; the reference is soft-deleted. The secret it pointed at is untouched"),
        (status = 403, description = "Missing integration:credential:delete"),
        (status = 404, description = "No such external system in this tenant, or no such credential on it")
    ),
    security(("bearer" = []))
)]
pub async fn delete_credential(
    State(state): State<AppState>,
    caller: Authenticated,
    PathParam((id, credential_id)): PathParam<(Uuid, Uuid)>,
) -> Result<StatusCode, AppError> {
    credential::delete_credential(&state, &caller, id, credential_id).await?;

    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// The integration log — under integration:log:read
// ---------------------------------------------------------------------------

/// List the integration log, newest first (FR-INT-006, #548).
///
/// Ordered by `startedAt` descending, then `id` descending. `from` is
/// inclusive and `to` exclusive. The payloads are not in a list item: they are
/// the detail's.
#[utoipa::path(
    get, path = "/api/v1/integration/logs", tag = "integration",
    params(IntegrationLogQuery),
    responses(
        (status = 200, description = "One page of the tenant's integration log rows, newest first", body = [IntegrationLogSummary]),
        (status = 403, description = "Missing integration:log:read"),
        (status = 422, description = "RANGE_INVERTED when `to` is before `from`; OUT_OF_RANGE on `from` or `to` when PostgreSQL cannot store the instant (before -4713-11-24T00:00:00Z); INVALID_CHARACTER on any parameter holding a NUL; INVALID_TYPE when a parameter will not parse")
    ),
    security(("bearer" = []))
)]
pub async fn list_logs(
    State(state): State<AppState>,
    caller: Authenticated,
    QueryParams(query): QueryParams<IntegrationLogQuery>,
) -> Result<Json<ListEnvelope<IntegrationLogSummary>>, AppError> {
    let (logs, meta) = log::list_logs(&state, &caller, &query).await?;

    Ok(Json(ListEnvelope::new(logs, meta)))
}

/// One integration log row whole. Its payloads are returned **exactly as
/// stored** — masked when they were written, and never resolved again.
#[utoipa::path(
    get, path = "/api/v1/integration/logs/{id}", tag = "integration",
    responses(
        (status = 200, description = "The log row, with its masked payloads as stored", body = IntegrationLog),
        (status = 403, description = "Missing integration:log:read"),
        (status = 404, description = "No such log row in this tenant")
    ),
    security(("bearer" = []))
)]
pub async fn get_log(
    State(state): State<AppState>,
    caller: Authenticated,
    PathParam(id): PathParam<Uuid>,
) -> Result<Json<ItemEnvelope<IntegrationLog>>, AppError> {
    Ok(Json(ItemEnvelope::new(
        log::get_log(&state, &caller, id).await?,
    )))
}
