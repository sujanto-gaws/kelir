use axum::routing::get;
use axum::{Json, Router};
use utoipa::OpenApi;

use crate::error::ValidationDetail;
use crate::health;
use crate::middleware::cors::cors_layer;
use crate::modules::{auth, identity, master_data};
use crate::response::{ErrorBody, ErrorEnvelope, PageMeta};
use crate::state::AppState;

/// The generated OpenAPI document (FR-API-004).
///
/// Never hand-edited (coding standard §2.6): endpoints are added by annotating
/// their handler with `#[utoipa::path]` and listing it here.
#[derive(OpenApi)]
#[openapi(
    paths(
        health::healthcheck,
        health::liveness,
        health::readiness,
        health::version,
        auth::handlers::sign_in,
        auth::handlers::refresh,
        auth::handlers::sign_out,
        auth::handlers::me,
        auth::handlers::change_password,
        identity::handlers::list_users,
        identity::handlers::get_user,
        identity::handlers::create_user,
        identity::handlers::update_user,
        identity::handlers::deactivate_user,
        identity::handlers::set_password,
        identity::handlers::list_roles,
        identity::handlers::get_role,
        identity::handlers::create_role,
        identity::handlers::update_role,
        identity::handlers::delete_role,
        identity::handlers::list_permissions,
        master_data::handlers::list_parties,
        master_data::handlers::get_party,
        master_data::handlers::create_party,
        master_data::handlers::update_party,
        master_data::handlers::delete_party,
        master_data::handlers::get_party_roles,
        master_data::handlers::assign_role,
        master_data::handlers::remove_role,
        master_data::handlers::list_suppliers,
        master_data::handlers::list_customers,
        master_data::handlers::list_employees,
    ),
    components(schemas(
        health::HealthBody,
        health::ReadyBody,
        health::VersionBody,
        auth::handlers::SignInRequest,
        auth::handlers::RefreshRequest,
        auth::handlers::SignOutRequest,
        auth::handlers::SessionResponse,
        auth::handlers::CurrentUser,
        auth::handlers::ChangePasswordRequest,
        identity::domain::User,
        identity::domain::UserStatus,
        identity::domain::RoleSummary,
        identity::domain::Role,
        identity::domain::Permission,
        identity::domain::CreateUserRequest,
        identity::domain::UpdateUserRequest,
        identity::domain::CreateRoleRequest,
        identity::domain::UpdateRoleRequest,
        identity::handlers::SetPasswordRequest,
        master_data::domain::PartyAggregate,
        master_data::domain::PartySummary,
        master_data::domain::PartyType,
        master_data::domain::PartyStatusCode,
        master_data::domain::Gender,
        master_data::domain::ContactMechType,
        master_data::domain::Person,
        master_data::domain::PartyGroup,
        master_data::domain::PartyIdentification,
        master_data::domain::PartyStatus,
        master_data::domain::PartyRelationship,
        master_data::domain::PartyClassification,
        master_data::domain::PartyContactMech,
        master_data::domain::ContactMechDetail,
        master_data::domain::PostalAddress,
        master_data::domain::TelecomNumber,
        master_data::domain::CreatePartyRequest,
        master_data::domain::UpdatePartyRequest,
        master_data::domain::PersonInput,
        master_data::domain::PartyGroupInput,
        master_data::domain::PartyIdentificationInput,
        master_data::domain::PartyRelationshipInput,
        master_data::domain::PartyClassificationInput,
        master_data::domain::PartyContactMechInput,
        master_data::domain::PartyRoles,
        master_data::domain::PartyRole,
        master_data::domain::PartyRoleStatus,
        master_data::domain::PartyProfiles,
        master_data::domain::SupplierProfile,
        master_data::domain::CustomerProfile,
        master_data::domain::EmployeeProfile,
        master_data::domain::ContactProfile,
        master_data::domain::SupplierApprovalStatus,
        master_data::domain::EmploymentType,
        master_data::domain::AssignRoleRequest,
        master_data::domain::RoleProfileInput,
        master_data::domain::SupplierProfileInput,
        master_data::domain::CustomerProfileInput,
        master_data::domain::EmployeeProfileInput,
        master_data::domain::ContactProfileInput,
        master_data::domain::RoleViewRow,
        ErrorEnvelope,
        ErrorBody,
        ValidationDetail,
        PageMeta,
    )),
    tags(
        (name = "operations", description = "Health, readiness and build information"),
        (name = "auth", description = "Sign in, sign out, session refresh"),
        (name = "identity", description = "Users, roles and permissions"),
        (name = "master-data", description = "Parties and their attributes")
    ),
    info(
        title = "Kelir API",
        description = "Metadata-driven, document-centric, workflow-enabled platform API.",
        version = env!("CARGO_PKG_VERSION"),
    )
)]
pub struct ApiDoc;

/// Builds the application router.
///
/// Operational endpoints stay at the root; everything else is versioned under
/// `/api/v1` (naming convention §5). Phase 2 mounts the module routers onto
/// `api_v1_router`.
pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health::healthcheck))
        .route("/health/live", get(health::liveness))
        .route("/health/ready", get(health::readiness))
        .route("/version", get(health::version))
        .route("/api/docs/openapi.json", get(openapi_document))
        .nest("/api/v1", api_v1_router(state.clone()))
        // Applied last so it wraps every route, including the 404 fallback and
        // the preflight requests the browser sends before anything else.
        .layer(cors_layer(&state.config.frontend_url))
        .with_state(state)
}

/// The versioned API surface. Module routers mount here as each is built.
///
/// Authentication is per-route rather than a blanket layer: `/auth/login` and
/// `/auth/refresh` must stay reachable without a token, and a handler that takes
/// `Authenticated` cannot be reached without one (FR-API-008). Making the rule
/// visible in each handler's signature beats a layer whose exceptions live
/// somewhere else.
///
/// The state is passed in as well as applied at the end, because the auth module
/// puts a stateful layer over its metered routes.
fn api_v1_router(state: AppState) -> Router<AppState> {
    Router::new()
        .nest("/auth", auth::handlers::routes(state))
        .nest("/identity", identity::handlers::routes())
        .nest("/master-data", master_data::handlers::routes())
}

async fn openapi_document() -> Json<utoipa::openapi::OpenApi> {
    Json(ApiDoc::openapi())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;
    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    fn test_state() -> AppState {
        let pool = crate::db::create_pool("postgres://postgres:postgres@localhost:5432/kelir")
            .expect("lazy pool builds without a server");

        AppState::new(pool, AppConfig::test_default())
    }

    async fn get(uri: &str) -> (StatusCode, serde_json::Value) {
        let response = create_router(test_state())
            .oneshot(
                Request::builder()
                    .uri(uri)
                    .body(Body::empty())
                    .expect("request builds"),
            )
            .await
            .expect("router responds");

        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body reads");
        let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);

        (status, json)
    }

    #[tokio::test]
    async fn serves_health() {
        let (status, body) = get("/health").await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["status"], "ok");
    }

    #[tokio::test]
    async fn serves_liveness_without_a_database() {
        // Liveness must not depend on PostgreSQL; no server is running here.
        let (status, body) = get("/health/live").await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["status"], "ok");
    }

    #[tokio::test]
    async fn serves_version() {
        let (status, body) = get("/version").await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["version"], health::VERSION);
        assert_eq!(body["environment"], "test");
    }

    #[tokio::test]
    async fn publishes_the_openapi_document() {
        let (status, body) = get("/api/docs/openapi.json").await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["info"]["title"], "Kelir API");
        assert!(
            body["paths"]["/health"].is_object(),
            "health is documented in the generated spec"
        );
    }

    #[tokio::test]
    async fn the_openapi_document_lists_every_auth_route() {
        // The Definition of Done says "API changes reflected in OpenAPI", and
        // `publishes_the_openapi_document` checks `/health` only — so the auth
        // surface could vanish from the document without a test noticing
        // (#60). Every path is listed, and the operation carries the method
        // that serves it.
        //
        // **What this cannot catch:** a route added to the router and
        // documented nowhere. `axum::Router` exposes no list of its paths, so
        // the expectation below is written by hand, and a new route reaches it
        // only when someone adds it. What it does catch is the reverse and more
        // likely direction — a documented route quietly losing its annotation.
        let (_, body) = get("/api/docs/openapi.json").await;

        let expected = [
            ("/api/v1/auth/login", "post"),
            ("/api/v1/auth/refresh", "post"),
            ("/api/v1/auth/logout", "post"),
            ("/api/v1/auth/me", "get"),
            ("/api/v1/auth/change-password", "post"),
        ];

        for (path, method) in expected {
            assert!(
                body["paths"][path][method].is_object(),
                "{method} {path} is missing from the published document"
            );
        }

        let documented: Vec<&str> = body["paths"]
            .as_object()
            .expect("paths is an object")
            .keys()
            .filter(|path| path.starts_with("/api/v1/auth/"))
            .map(String::as_str)
            .collect();

        assert_eq!(
            documented.len(),
            expected.len(),
            "the document has auth paths this test does not know about: {documented:?}"
        );
    }

    #[tokio::test]
    async fn the_openapi_document_lists_every_party_route() {
        // The Definition of Done says "API changes reflected in OpenAPI". The
        // party surface is the first module added since that document started
        // being checked, and a handler that loses its `#[utoipa::path]`
        // annotation still routes — it just stops existing for every client
        // generated from the spec.
        let (_, body) = get("/api/docs/openapi.json").await;

        let expected = [
            ("/api/v1/master-data/parties", "get"),
            ("/api/v1/master-data/parties", "post"),
            ("/api/v1/master-data/parties/{id}", "get"),
            ("/api/v1/master-data/parties/{id}", "put"),
            ("/api/v1/master-data/parties/{id}", "delete"),
            ("/api/v1/master-data/parties/{id}/roles", "get"),
            ("/api/v1/master-data/parties/{id}/roles/{roleTypeId}", "put"),
            (
                "/api/v1/master-data/parties/{id}/roles/{roleTypeId}",
                "delete",
            ),
            ("/api/v1/master-data/suppliers", "get"),
            ("/api/v1/master-data/customers", "get"),
            ("/api/v1/master-data/employees", "get"),
        ];

        for (path, method) in expected {
            assert!(
                body["paths"][path][method].is_object(),
                "{method} {path} is missing from the published document"
            );
        }

        // The role views are the API half of FR-MDM-008, so the parameters that
        // make them searchable have to be in the document a client generates
        // from: an endpoint published without them reads as a list that cannot
        // be filtered, and #101 is written against this spec.
        let parameters = body["paths"]["/api/v1/master-data/suppliers"]["get"]["parameters"]
            .as_array()
            .map(|parameters| {
                parameters
                    .iter()
                    .filter_map(|parameter| parameter["name"].as_str().map(str::to_owned))
                    .collect::<Vec<String>>()
            })
            .unwrap_or_default();

        for parameter in [
            "page",
            "pageSize",
            "search",
            "statusId",
            "partyTypeId",
            "roleStatusId",
        ] {
            assert!(
                parameters.iter().any(|name| name == parameter),
                "the supplier view is published without {parameter}: {parameters:?}"
            );
        }

        // The aggregate is the payload shape (architecture 05), so the schema a
        // client generates from has to carry its collections — a response type
        // trimmed to the party row would document a contract the API does not
        // serve.
        let aggregate = &body["components"]["schemas"]["PartyAggregate"]["properties"];
        for property in [
            "partyId",
            "partyTypeId",
            "identifications",
            "statuses",
            "relationshipsFrom",
            "relationshipsTo",
            "classifications",
            "contactMechanisms",
            "roles",
            "profiles",
        ] {
            assert!(
                aggregate[property].is_object(),
                "PartyAggregate is missing {property}: {aggregate}"
            );
        }
    }

    #[tokio::test]
    async fn assigning_a_role_documents_both_of_its_outcomes() {
        // `PUT` is idempotent here: the first call creates the assignment and
        // the rest update it. A generated client that only knew about 201 would
        // treat every repeat as a failure.
        let (_, body) = get("/api/docs/openapi.json").await;

        let responses = &body["paths"]["/api/v1/master-data/parties/{id}/roles/{roleTypeId}"]
            ["put"]["responses"];

        assert!(
            responses["201"].is_object(),
            "the created outcome is undocumented: {responses}"
        );
        assert!(
            responses["200"].is_object(),
            "the already-held outcome is undocumented: {responses}"
        );
    }

    #[tokio::test]
    async fn the_change_password_contract_does_not_promise_more_than_it_delivers() {
        // #60: the 204 read "every session for the account ends", while only
        // refresh tokens are revoked — false for up to fifteen minutes, in the
        // shared-machine case the doc comment gives as its justification. The
        // wording was narrowed rather than the behaviour changed
        // (architecture 01 §18.1 keeps authorization off the database), and
        // `an_access_token_issued_before_a_password_change_still_works` pins
        // the behaviour this description now matches.
        let (_, body) = get("/api/docs/openapi.json").await;

        let description = body["paths"]["/api/v1/auth/change-password"]["post"]["responses"]["204"]
            ["description"]
            .as_str()
            .expect("the 204 carries a description");

        assert!(
            description.contains("refresh token"),
            "the description no longer says which tokens are revoked: {description}"
        );
        assert!(
            !description.contains("every session"),
            "the overstated wording is back: {description}"
        );
    }

    #[tokio::test]
    async fn the_tenant_code_is_visible_in_the_published_contract() {
        // The reason tenancy travels in the request body rather than a header
        // (FR-IDM-009): a header carries the same trust as a body field while
        // being invisible to every client generated from this document. If the
        // field ever stops appearing here, that rationale has quietly lapsed.
        let (_, body) = get("/api/docs/openapi.json").await;

        let properties = &body["components"]["schemas"]["SignInRequest"]["properties"];

        assert!(
            properties["tenantCode"].is_object(),
            "tenantCode is missing from the published SignInRequest: {properties}"
        );

        // Optional in the contract, so single-tenant clients that never send it
        // remain conformant.
        let required = &body["components"]["schemas"]["SignInRequest"]["required"];
        assert!(
            !required
                .as_array()
                .is_some_and(|names| names.iter().any(|name| name == "tenantCode")),
            "tenantCode must stay optional; existing clients do not send it"
        );
    }

    #[tokio::test]
    async fn unknown_routes_are_not_found() {
        let (status, _) = get("/api/v1/does-not-exist").await;

        assert_eq!(status, StatusCode::NOT_FOUND);
    }
}
