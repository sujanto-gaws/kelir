//! An administrator's test call to one endpoint (FR-INT-002, #547; ADR-0043).
//!
//! **Synchronous, in the request, and nowhere else**: no outbox event, no hook
//! chain, no `core:` handler (ADR-0041 §6 stays untripped). The handler waits
//! for the call and answers with what came back.
//!
//! # One `integration_logs` row per call, and exactly one
//!
//! A call begins when its endpoint is found. A caller refused for permission,
//! or naming a system or endpoint that is not in their tenant, has made no
//! call and gets no row. Nor has a caller whose token is refused: a deleted
//! tenant's, or a deleted user's, is a 401 before this module is reached
//! (#650, D-105; it answers #648). **From there, every path ends in [`test_call`]'s one
//! insert**: [`attempt`] returns what happened, success or any failure, and
//! the row is written from that before the response is built. A refusal before
//! anything left the process — an inactive system, a `vault://` reference, an
//! address the guard refused — is a call that failed, and is logged as one.
//!
//! No transaction is held across the call: the row is one statement, written
//! after the call has finished, so a slow system holds a request worker for at
//! most its timeout and never a database connection.
//!
//! # No audit event
//!
//! A test call changes no configuration (coding standard §2.8 audits changes),
//! and its record is the `integration_logs` row.

use std::time::{Duration, Instant};

use chrono::Utc;
use serde_json::{json, Value};
use uuid::Uuid;

use super::system_not_found;
use crate::error::AppError;
use crate::middleware::auth::Authenticated;
use crate::modules::integration::domain::egress::EgressPolicy;
use crate::modules::integration::domain::external_system::{
    MAX_TIMEOUT_SECONDS, MIN_TIMEOUT_SECONDS,
};
use crate::modules::integration::domain::secret::{
    authorization_value, is_supported, redactions, HeaderError, TenantNamespaces,
};
use crate::modules::integration::domain::test_call::{
    choose_credential, preview, target_url, value_without_nul, without_nul, BodyPreview,
    TestCallError, TestCallResponse, TestCallStatus, REDACTED,
};
use crate::modules::integration::domain::{
    EndpointStatus, ExternalSystem, ExternalSystemStatus, IntegrationEndpoint,
};
use crate::modules::integration::outbound;
use crate::modules::integration::repository::log::{self as log_repo, NewIntegrationLog};
use crate::modules::integration::repository::{
    credential as credential_repo, endpoint as endpoint_repo, external_system as system_repo,
};
use crate::modules::integration::ENDPOINT_CALL;
use crate::modules::organization::service as organization;
use crate::state::AppState;

/// `integration_logs.entity_type` for a test call's row: the endpoint called.
pub const LOG_ENTITY_TYPE: &str = "IntegrationEndpoint";

/// What an attempt that got an answer brings back.
struct Answer {
    status_code: u16,
    preview: BodyPreview,
}

/// Why an attempt ended without an answer.
enum Failure {
    /// A failure the caller is told by name.
    Named(TestCallError),
    /// A failure of Kelir's own — the database, most likely. Logged as a
    /// failed call, answered as a 500.
    Internal(AppError),
}

impl From<TestCallError> for Failure {
    fn from(error: TestCallError) -> Self {
        Self::Named(error)
    }
}

/// What the log row says was sent. Filled in as the attempt gets further, so a
/// call refused at the secret still records the URL it was for.
struct Sent {
    url: Option<String>,
    credential: Option<(Uuid, &'static str)>,
    authorization: bool,
}

pub async fn test_call(
    state: &AppState,
    caller: &Authenticated,
    system_id: Uuid,
    endpoint_id: Uuid,
) -> Result<TestCallResponse, AppError> {
    caller.require(ENDPOINT_CALL)?;

    let tenant_id = caller.tenant_id();

    let system = system_repo::find_external_system(&state.pool, tenant_id, system_id)
        .await?
        .ok_or_else(system_not_found)?;
    let endpoint = endpoint_repo::find_endpoint(&state.pool, tenant_id, system_id, endpoint_id)
        .await?
        .ok_or_else(|| AppError::not_found("Integration endpoint"))?;

    // The call begins here: everything below writes its one row.
    let log_id = Uuid::now_v7();
    let correlation_id = Uuid::now_v7().to_string();
    let started_at = Utc::now();
    let clock = Instant::now();

    let mut sent = Sent {
        url: None,
        credential: None,
        authorization: false,
    };
    let outcome = attempt(
        state,
        tenant_id,
        &system,
        &endpoint,
        &correlation_id,
        &mut sent,
    )
    .await;

    let completed_at = Utc::now();
    let duration_ms = i64::try_from(clock.elapsed().as_millis()).unwrap_or(i64::MAX);

    let (status, status_code, response_payload, error_message) = match &outcome {
        Ok(answer) => (
            TestCallStatus::for_status_code(answer.status_code).as_db(),
            Some(i32::from(answer.status_code)),
            Some(json!({
                "statusCode": answer.status_code,
                "bodyPreview": answer.preview.text,
                "bodyTruncated": answer.preview.truncated,
            })),
            None,
        ),
        Err(Failure::Named(error)) => (
            "FAILED",
            None,
            None,
            // A secret's environment-variable name comes from a stored
            // reference, and a `text` column refuses U+0000.
            Some(without_nul(&format!("{}: {}", error.code(), error.message())).into_owned()),
        ),
        Err(Failure::Internal(_)) => (
            "FAILED",
            None,
            None,
            Some("INTERNAL_ERROR: the call could not be completed".to_owned()),
        ),
    };

    // PostgreSQL refuses U+0000 in `jsonb` and `text`, so a system that
    // answered with one would otherwise cost the call its row (AC6). The
    // preview has none already; this is the net under everything else.
    let request_payload = value_without_nul(request_record(&endpoint, &sent, &correlation_id));
    let response_payload = response_payload.map(value_without_nul);

    let written = log_repo::insert_log(
        &state.pool,
        &NewIntegrationLog {
            id: log_id,
            tenant_id,
            external_system_id: system.id,
            integration_type: "REST",
            endpoint: sent.url.as_deref(),
            method: endpoint.method.as_db(),
            correlation_id: &correlation_id,
            entity_type: LOG_ENTITY_TYPE,
            entity_id: endpoint.id,
            request_payload_json: Some(request_payload),
            response_payload_json: response_payload,
            status_code,
            status,
            error_message: error_message.as_deref(),
            started_at,
            completed_at,
            duration_ms: i32::try_from(duration_ms).unwrap_or(i32::MAX),
        },
    )
    .await;

    if let Err(error) = written {
        // The call was made — perhaps with the credential attached — and its
        // record is lost. Said here, at error level and with the ids an
        // operator needs, before the generic 500 discards the context.
        tracing::error!(
            %log_id,
            system_id = %system.id,
            endpoint_id = %endpoint.id,
            %correlation_id,
            status,
            status_code,
            authorization_sent = sent.authorization,
            error = %error,
            "an integration test call's log row could not be written"
        );
        return Err(error.into());
    }

    tracing::info!(
        %log_id,
        system_id = %system.id,
        endpoint_id = %endpoint.id,
        status,
        status_code,
        duration_ms,
        "integration test call"
    );

    match outcome {
        Ok(answer) => Ok(TestCallResponse {
            log_id,
            method: endpoint.method,
            url: sent.url.unwrap_or_default(),
            status: TestCallStatus::for_status_code(answer.status_code),
            status_code: answer.status_code,
            duration_ms,
            body_preview: answer.preview.text,
            body_truncated: answer.preview.truncated,
        }),
        Err(Failure::Named(error)) => Err(error.into_app_error(log_id)),
        Err(Failure::Internal(error)) => Err(error),
    }
}

/// Everything between finding the endpoint and writing its row.
///
/// Order matters, and it is chosen so that each refusal happens before the
/// step it protects: nothing is resolved for a system that is off, no secret
/// is read for a type that is not built, and nothing is sent before the
/// address has passed the guard.
async fn attempt(
    state: &AppState,
    tenant_id: Uuid,
    system: &ExternalSystem,
    endpoint: &IntegrationEndpoint,
    correlation_id: &str,
    sent: &mut Sent,
) -> Result<Answer, Failure> {
    if system.status != ExternalSystemStatus::Active {
        return Err(TestCallError::SystemNotActive.into());
    }
    if endpoint.status != EndpointStatus::Active {
        return Err(TestCallError::EndpointNotActive.into());
    }

    let base_url = system
        .base_url
        .as_deref()
        .ok_or(TestCallError::BaseUrlMissing)?;
    let url = target_url(base_url, &endpoint.path)?;
    sent.url = Some(url.to_string());

    let credentials = credential_repo::active_credentials(&state.pool, tenant_id, system.id)
        .await
        .map_err(|error| Failure::Internal(error.into()))?;
    let credential = choose_credential(&credentials, Utc::now().date_naive())?;
    sent.credential = Some((credential.id, credential.credential_type.as_db()));

    if !is_supported(credential.credential_type) {
        return Err(TestCallError::CredentialTypeNotSupported(credential.credential_type).into());
    }

    // The caller's tenant code, and every other live tenant's, in one read
    // and before the environment is (#618). From the caller's `tenant_id`,
    // never from the request, the token or the reference.
    //
    // **The "not live" branch below is a race and nothing else.** A deleted
    // tenant's token is refused by `middleware::auth` before this is reached
    // (#650), so the caller's tenant is missing here only if it was deleted
    // between that read and this one. No request can stage it; it fails
    // closed, as a 500 with a `FAILED` row, and resolves nothing. **No test
    // holds this branch**: `domain::secret`'s unit test holds that
    // `TenantNamespaces::for_caller` answers `None`, and what is done with
    // that `None` here is held by review (`tests/integration_test_call.rs`'s
    // header records the mutation that stays green).
    let live = organization::live_tenant_codes(&state.pool)
        .await
        .map_err(Failure::Internal)?;
    let namespaces = TenantNamespaces::for_caller(tenant_id, live).ok_or_else(|| {
        Failure::Internal(
            anyhow::anyhow!("the caller's tenant is not live, so it has no secret namespace")
                .into(),
        )
    })?;
    let secret = outbound::resolve_secret(&credential.secret_reference, &namespaces)?;
    let header = authorization_value(credential.credential_type, &secret).map_err(|error| {
        TestCallError::SecretMalformed(match error {
            HeaderError::BasicWithoutSeparator => {
                "is not a user:password pair, which a BASIC_AUTH credential must be"
            }
            HeaderError::NotHeaderSafe => "contains a character a header cannot carry",
            HeaderError::Empty => "is empty, or has an empty user name",
            HeaderError::NotSupported(_) => "is of a type a test call does not attach",
        })
    })?;
    let redactions = redactions(credential.credential_type, &secret);
    drop(secret);

    let seconds = system
        .timeout_seconds
        .clamp(MIN_TIMEOUT_SECONDS, MAX_TIMEOUT_SECONDS);
    let budget = Duration::from_secs(u64::try_from(seconds).unwrap_or(30));
    let timed_out = TestCallError::UpstreamTimeout { seconds };

    let policy = EgressPolicy {
        allowed_cidrs: state.config.integration_allowed_cidrs.clone(),
        allow_loopback: state.config.integration_allow_loopback,
    };

    let lookup = outbound::Lookup {
        overrides: &state.config.integration_dns_overrides,
    };

    // One deadline over resolution, the request and the body read: the
    // system's timeout is the whole call's, not each step's.
    let call = async {
        let pinned = outbound::resolve_and_check(&url, &policy, lookup).await?;
        sent.authorization = true;
        outbound::send(
            endpoint.method,
            &url,
            &pinned,
            Some(&header),
            correlation_id,
            budget,
        )
        .await
    };

    let raw = match tokio::time::timeout(budget, call).await {
        Ok(Ok(raw)) => raw,
        Ok(Err(TestCallError::UpstreamTimeout { .. })) | Err(_) => {
            return Err(timed_out.into());
        }
        Ok(Err(other)) => return Err(other.into()),
    };

    Ok(Answer {
        status_code: raw.status_code,
        preview: preview(&raw.body, &redactions, raw.body_cut),
    })
}

/// `request_payload_json`: what was sent, masked. The `Authorization` header
/// is recorded as present and never as its value.
fn request_record(endpoint: &IntegrationEndpoint, sent: &Sent, correlation_id: &str) -> Value {
    let mut headers = serde_json::Map::new();
    headers.insert(
        outbound::CORRELATION_HEADER.to_owned(),
        Value::String(correlation_id.to_owned()),
    );
    if sent.authorization {
        headers.insert(
            "Authorization".to_owned(),
            Value::String(REDACTED.to_owned()),
        );
    }

    json!({
        "method": endpoint.method.as_db(),
        "url": sent.url,
        "headers": headers,
        "body": Value::Null,
        "credentialId": sent.credential.map(|(id, _)| id),
        "credentialType": sent.credential.map(|(_, kind)| kind),
    })
}
