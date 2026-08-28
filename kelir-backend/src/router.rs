use axum::routing::get;
use axum::{Json, Router};
use utoipa::OpenApi;

use crate::error::ValidationDetail;
use crate::health;
use crate::middleware::cors::cors_layer;
use crate::modules::{
    auth, document, document_type, identity, master_data, organization, rad, task_inbox, workflow,
};
use crate::response::{ErrorBody, ErrorEnvelope, PageMeta};
use crate::state::AppState;

/// The generated OpenAPI document (FR-API-004).
///
/// Never hand-edited (coding standard §2.6): endpoints are added by annotating
/// their handler with `#[utoipa::path]` **and** listing it here. Both halves
/// are load-bearing and only one of them is visible from the handler: an
/// annotation whose handler is missing from `paths(...)` compiles, routes,
/// serves traffic, and reaches no generated client. Nine of them did, for a
/// whole sprint (#138).
///
/// `every_annotated_route_reaches_the_document` is what now holds the two
/// halves together. It reads the source rather than a list of routes, because
/// the test it replaced was a list of routes and aged out of usefulness while
/// still passing.
#[derive(OpenApi)]
#[openapi(
    paths(
        health::healthcheck,
        health::liveness,
        health::readiness,
        health::version,
        health::deployment,
        auth::handlers::sign_in,
        auth::handlers::refresh,
        auth::handlers::sign_out,
        auth::handlers::me,
        auth::handlers::change_password,
        auth::handlers::forgot_password,
        auth::handlers::reset_password,
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
        master_data::handlers::list_facilities,
        master_data::handlers::get_facility,
        master_data::handlers::create_facility,
        master_data::handlers::update_facility,
        master_data::handlers::delete_facility,
        master_data::handlers::transition_party,
        master_data::handlers::transition_facility,
        master_data::handlers::party_audit,
        master_data::handlers::facility_audit,
        rad::handlers::list_forms,
        rad::handlers::get_form,
        rad::handlers::create_form,
        rad::handlers::update_form,
        rad::handlers::publish_form,
        rad::handlers::create_revision,
        rad::handlers::submit_form,
        rad::handlers::delete_form,
        rad::handlers::list_lists,
        rad::handlers::get_list,
        rad::handlers::create_list,
        rad::handlers::update_list,
        rad::handlers::delete_list,
        rad::handlers::list_lookup_options,
        document_type::handlers::list_types,
        document_type::handlers::get_type,
        document_type::handlers::create_type,
        document_type::handlers::update_type,
        document_type::handlers::delete_type,
        document_type::handlers::get_numbering_rule,
        document_type::handlers::set_numbering_rule,
        document_type::handlers::clear_numbering_rule,
        document::handlers::list_documents,
        document::handlers::get_document,
        document::handlers::create_document,
        document::handlers::update_document,
        document::handlers::delete_document,
        document::handlers::submit_document,
        document::handlers::transition_document,
        document::handlers::status_history,
        document::handlers::resolve_linked_entity,
        workflow::handlers::list_definitions,
        workflow::handlers::get_definition,
        workflow::handlers::create_definition,
        workflow::handlers::update_definition,
        workflow::handlers::publish_definition,
        workflow::handlers::create_revision,
        workflow::handlers::delete_definition,
        workflow::handlers::get_instance,
        workflow::handlers::claim_task,
        workflow::handlers::decide_task,
        document::handlers::document_workflow,
        document::handlers::document_workflow_history,
        task_inbox::handlers::list_tasks,
        task_inbox::handlers::get_task,
        organization::handlers::list_departments,
        organization::handlers::get_department,
        organization::handlers::create_department,
        organization::handlers::update_department,
        organization::handlers::delete_department,
        organization::handlers::list_tenants,
        organization::handlers::get_tenant,
        organization::handlers::create_tenant,
        organization::handlers::update_tenant,
        organization::handlers::delete_tenant,
    ),
    components(schemas(
        health::HealthBody,
        health::ReadyBody,
        health::VersionBody,
        health::DeploymentBody,
        auth::handlers::SignInRequest,
        auth::handlers::RefreshRequest,
        auth::handlers::SignOutRequest,
        auth::handlers::SessionResponse,
        auth::handlers::CurrentUser,
        auth::handlers::ChangePasswordRequest,
        auth::reset::RequestResetRequest,
        auth::reset::ResetPasswordRequest,
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
        master_data::domain::AssignedRole,
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
        master_data::domain::Facility,
        master_data::domain::FacilitySummary,
        master_data::domain::FacilityType,
        master_data::domain::CreateFacilityRequest,
        master_data::domain::UpdateFacilityRequest,
        master_data::domain::RecordStatus,
        master_data::domain::TransitionRequest,
        master_data::domain::TransitionResult,
        master_data::domain::AuditRecord,
        rad::domain::Form,
        rad::domain::FormSummary,
        rad::domain::FormStatus,
        rad::domain::CreateFormRequest,
        rad::domain::UpdateFormRequest,
        rad::domain::ListDefinition,
        rad::domain::ListSummary,
        rad::domain::ListStatus,
        rad::domain::ListColumnInput,
        rad::domain::ListFilterInput,
        rad::domain::list::FilterType,
        rad::domain::CreateListRequest,
        rad::domain::UpdateListRequest,
        rad::domain::LookupOption,
        rad::domain::submission::Submission,
        rad::domain::submission::SubmitFormRequest,
        document_type::domain::DocumentType,
        document_type::domain::DocumentTypeSummary,
        document_type::domain::DocumentTypeStatus,
        document_type::domain::SecurityLevel,
        document_type::domain::WorkflowBinding,
        document_type::domain::CreateDocumentTypeRequest,
        document_type::domain::UpdateDocumentTypeRequest,
        document_type::numbering::NumberingRule,
        document_type::numbering::SetNumberingRuleRequest,
        document_type::numbering::SequenceScope,
        document_type::numbering::GapPolicy,
        document::domain::Document,
        document::domain::DocumentSummary,
        document::domain::DocumentStatus,
        document::domain::DocumentPriority,
        document::domain::CreateDocumentRequest,
        document::domain::UpdateDocumentRequest,
        document::domain::TransitionRequest,
        document::domain::TransitionResult,
        document::domain::EntityType,
        document::domain::EntityLink,
        document::domain::ResolvedEntity,
        document::domain::MetadataEntry,
        document::domain::MetadataType,
        document::service::StatusHistoryEntry,
        workflow::domain::WorkflowDefinition,
        workflow::domain::WorkflowDefinitionSummary,
        workflow::domain::WorkflowDefinitionStatus,
        workflow::domain::CreateWorkflowRequest,
        workflow::domain::UpdateWorkflowRequest,
        workflow::domain::WorkflowInstance,
        workflow::domain::WorkflowVariable,
        workflow::domain::InstanceStatus,
        workflow::domain::InstanceOutcome,
        workflow::domain::WorkflowTask,
        workflow::domain::TaskStatus,
        workflow::domain::DecisionAction,
        workflow::domain::DecisionRequest,
        workflow::domain::Assignment,
        workflow::service::instance::DocumentWorkflow,
        workflow::domain::WorkflowHistoryEntry,
        workflow::service::task::DecisionResult,
        workflow::service::inbox::InboxTask,
        workflow::service::inbox::TaskDetail,
        workflow::service::inbox::AvailableDecision,
        organization::department::Department,
        organization::department::DepartmentStatus,
        organization::department::CreateDepartmentRequest,
        organization::department::UpdateDepartmentRequest,
        organization::domain::TenantView,
        organization::domain::TenantStatus,
        organization::domain::CreateTenantRequest,
        organization::domain::TenantAdministratorInput,
        organization::domain::UpdateTenantRequest,
        ErrorEnvelope,
        ErrorBody,
        ValidationDetail,
        PageMeta,
    )),
    tags(
        (name = "operations", description = "Health, readiness and build information"),
        (name = "auth", description = "Sign in, sign out, session refresh"),
        (name = "identity", description = "Users, roles and permissions"),
        (
            name = "organization",
            description = "Tenants, departments, and the organizational structure a user or an employee belongs to. Tenant routes are administrable only from the deployment's default tenant"
        ),
        (
            name = "document-type",
            description = "Document types and the form, list and workflow bindings that configure them"
        ),
        (
            name = "document",
            description = "Documents — created from a type, filled through its form, submitted with a number, and moved through their own statuses"
        ),
        (
            name = "workflow",
            description = "Workflow definitions, the processes running against them, and the tasks they generate"
        ),
        (
            name = "task",
            description = "The caller's own task inbox — what is waiting for them, and what one task is asking"
        ),
        (
            name = "rad",
            description = "Form and list definitions — the metadata a document type binds and a renderer reads"
        ),
        (
            name = "master-data",
            description = "Parties, facilities, their governance lifecycle and their change history"
        )
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
        // Operational, and at the root beside `/version` for the same reason:
        // it identifies the deployment rather than serving its data, and it
        // answers outside the response envelope. Unauthenticated because the
        // login form reads it before there is anything to authenticate with
        // (#67, decision D-18). See `health::deployment`.
        .route("/deployment", get(health::deployment))
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
        .nest("/rad", rad::handlers::routes())
        .nest("/document-types", document_type::handlers::routes())
        .nest("/documents", document::handlers::routes())
        .nest("/workflow", workflow::handlers::routes())
        .nest("/tasks", task_inbox::handlers::routes())
        .nest("/organization", organization::handlers::routes())
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
            ("/api/v1/auth/forgot-password", "post"),
            ("/api/v1/auth/reset-password", "post"),
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

    /// Every route the source annotates reaches the published document, and
    /// every route the source serves is annotated (#138).
    ///
    /// **This test names no routes, because the test it replaces did.** Eleven
    /// party paths were listed here and asserted for a whole sprint while nine
    /// routes it had never heard of — every facility route, both lifecycle
    /// transitions and both change-history routes — reached no generated client
    /// at all. A `#[utoipa::path]` annotation is inert unless the handler is
    /// also listed in `paths(...)` above, nothing warns when it is not, and a
    /// checklist of routes ages exactly as fast as the list it is checking.
    ///
    /// So it reads the source instead. Both directions matter and they catch
    /// different mistakes: annotate-and-forget-to-register is what happened,
    /// and route-without-annotating is the way the same hole opens from the
    /// other side.
    ///
    /// Two literals are skipped, and both are named here rather than filtered
    /// by a pattern that might quietly grow: `/api/docs/openapi.json` serves the
    /// document itself and is not part of the API it describes, and `"/"`
    /// appears only in `extract.rs`'s own test scaffolding.
    #[tokio::test]
    async fn every_annotated_route_reaches_the_document() {
        use std::collections::BTreeSet;

        let (_, body) = get("/api/docs/openapi.json").await;
        let documented: BTreeSet<String> = body["paths"]
            .as_object()
            .map(|paths| paths.keys().cloned().collect())
            .unwrap_or_default();

        let sources = rust_sources("src".as_ref());
        assert!(
            sources.len() > 10,
            "the source scan found {} files, so it is not reading the crate",
            sources.len()
        );

        let mut annotated = BTreeSet::new();
        let mut served = BTreeSet::new();

        for text in &sources {
            for capture in between(text, "#[utoipa::path", "]") {
                if let Some(path) = quoted_after(&capture, "path = ") {
                    annotated.insert(path);
                }
            }

            for capture in between(text, ".route(", ")") {
                if let Some(path) = first_quoted(&capture) {
                    served.insert(path);
                }
            }
        }

        served.remove("/api/docs/openapi.json");
        served.remove("/");

        assert!(
            !annotated.is_empty() && !served.is_empty(),
            "the scan found no routes at all — annotated: {annotated:?}, served: {served:?}"
        );

        let undocumented: Vec<&String> = annotated
            .iter()
            .filter(|path| !documented.contains(*path))
            .collect();
        assert!(
            undocumented.is_empty(),
            "annotated but absent from the published document — the handler is \
             probably missing from `paths(...)`: {undocumented:?}"
        );

        // A served route is mounted under a prefix its annotation spells out in
        // full, so the annotation ends with the literal the router registers.
        let unannotated: Vec<&String> = served
            .iter()
            .filter(|literal| {
                !annotated
                    .iter()
                    .any(|path| path.ends_with(literal.as_str()))
            })
            .collect();
        assert!(
            unannotated.is_empty(),
            "served but carrying no `#[utoipa::path]` annotation: {unannotated:?}"
        );
    }

    /// Every `.rs` file under a directory, read.
    fn rust_sources(dir: &std::path::Path) -> Vec<String> {
        let mut out = Vec::new();
        let entries = std::fs::read_dir(dir)
            .unwrap_or_else(|error| panic!("reading {}: {error}", dir.display()));

        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                out.extend(rust_sources(&path));
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                out.push(std::fs::read_to_string(&path).unwrap_or_default());
            }
        }

        out
    }

    /// Each slice of `text` that starts after `open` and ends before the next
    /// `close`.
    ///
    /// A needle immediately preceded by a double quote is skipped: this file
    /// contains the scanner, so it contains the literals the scanner looks for,
    /// and without this the test reports its own source as an undocumented
    /// route. That is a real hazard of scanning source rather than a nuisance —
    /// it is the first thing this test did.
    fn between(text: &str, open: &str, close: &str) -> Vec<String> {
        let mut out = Vec::new();
        let bytes = text.as_bytes();
        let mut offset = 0;

        while let Some(found) = text[offset..].find(open) {
            let start = offset + found;
            let after = start + open.len();
            let end = text[after..]
                .find(close)
                .map_or(text.len(), |index| after + index);

            if !(start > 0 && bytes[start - 1] == b'"') {
                out.push(text[after..end].to_owned());
            }

            offset = end.max(after);
        }

        out
    }

    /// The first double-quoted string in `text`.
    fn first_quoted(text: &str) -> Option<String> {
        let start = text.find('"')? + 1;
        let end = text[start..].find('"')? + start;

        Some(text[start..end].to_owned())
    }

    /// The double-quoted string that follows `key`.
    fn quoted_after(text: &str, key: &str) -> Option<String> {
        let at = text.find(key)? + key.len();

        first_quoted(&text[at..])
    }

    #[tokio::test]
    async fn the_master_data_document_carries_the_shapes_a_client_needs() {
        // Which master-data *routes* are published is
        // `every_annotated_route_reaches_the_document`'s job, and it does not
        // need updating when one is added. What is left here is what only this
        // surface can assert: that the published operations carry the query
        // parameters and the response shape the API actually serves.
        //
        // This test used to name eleven party routes. That enumeration passed
        // for the whole of Sprint 6 while nine routes it did not know about
        // reached no client at all (#138) — a list of routes has the same
        // failure mode as the list it is checking.
        let (_, body) = get("/api/docs/openapi.json").await;

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

        // And both outcomes answer with the assignment this call stated, not
        // with every role and profile the party holds and not with the stored
        // row. The published shape is the contract a client is generated from,
        // so a response documented as `PartyRoles` would have clients expecting
        // the bank accounts #104 took out of it, and one documented as
        // `PartyRole` would have them expecting the merged `comments` and
        // `additionalAttributes` #119 took out after it. Either would be the
        // first sign of the leak coming back.
        for outcome in ["200", "201"] {
            let schema = &responses[outcome]["content"]["application/json"]["schema"];
            let referenced = schema["$ref"].as_str().unwrap_or_default();

            assert!(
                referenced.ends_with("/AssignedRole"),
                "{outcome} is documented as {schema}, not as the assignment it returns"
            );
        }
    }

    #[tokio::test]
    async fn assigning_a_role_publishes_which_fields_it_replaces_and_which_it_merges() {
        // #120. `update_party_role` treats five columns two ways in one
        // statement, deliberately and for a good reason: `starts_at`/`ends_at`
        // are the assignment's period, a PUT states the period, and a
        // `thruDate` that could be set but never cleared would make an ended
        // role impossible to reopen. The reason is sound and was written down —
        // in a doc comment on the repository function. **No caller can read
        // that.** The behaviour was discoverable only by losing a `thruDate`.
        //
        // This asserts the published contract carries it. The behaviour itself
        // is pinned by
        // `a_restatement_that_omits_everything_optional_clears_the_end_date_and_keeps_the_rest`
        // in `master_data_party_roles.rs`; the two together are what stops the
        // contract and the code drifting apart.
        let (_, body) = get("/api/docs/openapi.json").await;

        let description = body["paths"]["/api/v1/master-data/parties/{id}/roles/{roleTypeId}"]
            ["put"]["description"]
            .as_str()
            .expect("the operation carries a description");

        for field in ["fromDate", "thruDate"] {
            assert!(
                description.contains(field),
                "the contract does not name {field} as replaced: {description}"
            );
        }
        for field in ["statusId", "comments", "additionalAttributes"] {
            assert!(
                description.contains(field),
                "the contract does not name {field} as merged: {description}"
            );
        }
        assert!(
            description.contains("replaced") && description.contains("merged"),
            "the contract does not say which fields are which: {description}"
        );
        assert!(
            description.contains("clears it"),
            "the contract does not say what omitting thruDate does: {description}"
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
