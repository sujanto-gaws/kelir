//! Routes under `/api/v1/integration/external-systems` (FR-INT-001, #520).
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
    ExternalSystemQuery, IntegrationCredential, IntegrationEndpoint, RegisterExternalSystemRequest,
    UpdateExternalSystemRequest, UpdateIntegrationCredentialRequest,
    UpdateIntegrationEndpointRequest,
};
use super::service::{credential, endpoint, external_system};
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
            "/external-systems/{id}/credentials",
            get(list_credentials).post(create_credential),
        )
        .route(
            "/external-systems/{id}/credentials/{credentialId}",
            get(get_credential)
                .put(update_credential)
                .delete(delete_credential),
        )
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
        (status = 422, description = "Validation failed — including a baseUrl carrying a user name or password (CREDENTIALS_IN_URL)")
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
        (status = 422, description = "Validation failed — NOT_ALLOWED on status when the edit would move the system into or out of INACTIVE")
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

// ---------------------------------------------------------------------------
// Credential references — under integration:credential:*
// ---------------------------------------------------------------------------

#[utoipa::path(
    get, path = "/api/v1/integration/external-systems/{id}/credentials", tag = "integration",
    params(Pagination),
    responses(
        (status = 200, description = "One page of the system's credential references, inactive ones included. References only — never a secret", body = [IntegrationCredential]),
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
        (status = 200, description = "The credential reference — never the secret", body = IntegrationCredential),
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
