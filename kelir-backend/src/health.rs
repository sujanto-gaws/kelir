use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::Serialize;
use utoipa::ToSchema;

use crate::db;
use crate::state::AppState;

/// Build version, reported by `/version`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Commit the binary was built from, baked in by `build.rs`. The release
/// smoke test checks this against the tag being released (release process §4).
pub const BUILD_SHA: &str = env!("KELIR_BUILD_SHA");

#[derive(Debug, Serialize, ToSchema)]
pub struct HealthBody {
    /// `ok` when serving, `degraded` when a dependency is unavailable.
    pub status: &'static str,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReadyBody {
    pub status: &'static str,
    pub database: &'static str,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct VersionBody {
    pub name: String,
    pub version: &'static str,
    pub commit: &'static str,
    pub environment: String,
}

/// What a client needs to know before anybody has signed in.
///
/// Exactly one field today, and the field is the point: the login form cannot
/// know whether to ask for a tenant code unless the deployment tells it
/// (#67, decision **D-18**).
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeploymentBody {
    /// Whether this deployment serves more than one tenant, and therefore
    /// whether `POST /auth/login` requires `tenantCode`.
    pub multi_tenant: bool,
}

/// Overall health. Kept dependency-free so it answers even while PostgreSQL is
/// still starting; `/health/ready` is the probe that gates traffic.
#[utoipa::path(
    get,
    path = "/health",
    tag = "operations",
    responses((status = 200, description = "The process is serving", body = HealthBody))
)]
pub async fn healthcheck() -> Json<HealthBody> {
    Json(HealthBody { status: "ok" })
}

/// Liveness: is the process running at all? Never touches dependencies — a
/// failing liveness probe means "restart me", which a database outage is not.
#[utoipa::path(
    get,
    path = "/health/live",
    tag = "operations",
    responses((status = 200, description = "The process is alive", body = HealthBody))
)]
pub async fn liveness() -> Json<HealthBody> {
    Json(HealthBody { status: "ok" })
}

/// Readiness: can this instance serve requests? Checks the database and
/// answers 503 when it cannot, so a load balancer stops sending traffic
/// without the container being killed (NFR-AVA-002).
#[utoipa::path(
    get,
    path = "/health/ready",
    tag = "operations",
    responses(
        (status = 200, description = "Dependencies are reachable", body = ReadyBody),
        (status = 503, description = "A dependency is unavailable", body = ReadyBody)
    )
)]
pub async fn readiness(State(state): State<AppState>) -> (StatusCode, Json<ReadyBody>) {
    match db::ping(&state.pool).await {
        Ok(()) => (
            StatusCode::OK,
            Json(ReadyBody {
                status: "ok",
                database: "up",
            }),
        ),
        Err(error) => {
            tracing::warn!(error = ?error, "readiness probe failed: database unreachable");

            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(ReadyBody {
                    status: "degraded",
                    database: "down",
                }),
            )
        }
    }
}

/// Build and environment identification.
#[utoipa::path(
    get,
    path = "/version",
    tag = "operations",
    responses((status = 200, description = "Build information", body = VersionBody))
)]
pub async fn version(State(state): State<AppState>) -> Json<VersionBody> {
    Json(VersionBody {
        name: state.config.app_name.clone(),
        version: VERSION,
        commit: BUILD_SHA,
        environment: state.config.app_env.to_string(),
    })
}

/// How this deployment is configured, for a client that has not signed in yet.
///
/// **Unauthenticated on purpose, and this is the decision #67 asked for.** The
/// login form has to know whether to show a tenant-code field before it has any
/// credentials to show it with, so the answer cannot sit behind a token. Two
/// transports were available:
///
/// - a build-time `VITE_KELIR_MULTI_TENANT` flag, which is cheaper and would
///   couple the frontend *image* to one backend's setting. The image today
///   bakes only `VITE_KELIR_API_BASE_URL=/api/v1`, a relative path, so one
///   build serves every deployment — a build-time tenancy flag would end that
///   and make the image per-deployment.
/// - this endpoint, which is new API surface and keeps the image generic.
///
/// **D-18** takes the second. What it discloses is one boolean that the login
/// form is about to make visible anyway, and nothing about which tenants exist:
/// enumerating those needs `organization:tenant:read` and the administering
/// tenant.
///
/// Not rate-limited, for the same reason `/health` is not: it reads no
/// database, costs a struct, and reveals nothing worth guessing at.
#[utoipa::path(
    get,
    path = "/deployment",
    tag = "operations",
    responses((status = 200, description = "How this deployment is configured", body = DeploymentBody))
)]
pub async fn deployment(State(state): State<AppState>) -> Json<DeploymentBody> {
    Json(DeploymentBody {
        multi_tenant: state.config.multi_tenant,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn health_reports_ok() {
        let Json(body) = healthcheck().await;

        assert_eq!(body.status, "ok");
    }

    #[tokio::test]
    async fn liveness_reports_ok_without_dependencies() {
        let Json(body) = liveness().await;

        assert_eq!(body.status, "ok");
    }

    #[test]
    fn version_is_the_crate_version() {
        assert_eq!(VERSION, env!("CARGO_PKG_VERSION"));
        assert!(!VERSION.is_empty());
    }

    #[test]
    fn build_sha_is_always_populated() {
        // The release smoke test compares this against the released tag, so an
        // empty value would make that check silently pass.
        assert!(!BUILD_SHA.is_empty());
    }

    #[tokio::test]
    async fn deployment_reports_the_tenancy_mode_it_is_configured_with() {
        // The login form renders its tenant-code field off this and nothing
        // else, so a value that did not follow the configuration would produce
        // exactly the unrecoverable sign-in #67 was filed about — from the
        // other direction.
        for multi_tenant in [false, true] {
            let config = crate::config::AppConfig {
                multi_tenant,
                ..crate::config::AppConfig::test_default()
            };
            let pool = crate::db::create_pool("postgres://postgres:postgres@localhost:5432/kelir")
                .expect("lazy pool builds without a server");

            let Json(body) = deployment(State(AppState::new(pool, config))).await;

            assert_eq!(body.multi_tenant, multi_tenant);
        }
    }
}
