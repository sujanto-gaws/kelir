//! The `after_workflow_transition` chain, as the outbox's consumer of
//! `Workflow.Transitioned` ([#519]; ADR-0041 §2).
//!
//! # The chain is resolved at delivery
//!
//! The event carries the envelope and nothing else — EES S4 makes it the bytes
//! a webhook will receive, so a handler list inside it would be data no
//! webhook consumer asked for. What runs is worked out here, when the event is
//! delivered, from two sources merged by LHCS §3.1's priority:
//!
//! - **the transition's own `actions`**, read from the definition revision the
//!   instance runs. The revision is pinned on the instance (architectures/01
//!   §12.5), so these are the actions that were in force at the transition,
//!   however many revisions have been published since;
//! - **the registry's `after_workflow_transition` entries** for the document's
//!   type and the tenant-wide ones, **as they stand at delivery**. An entry
//!   added between commit and delivery runs, and one removed does not. That is
//!   ADR-0041's stated property, not an accident of the order things happen in.
//!
//! # The payload is the document at delivery
//!
//! LHCS §4's `formData`, `metadata` and `currentStatus` are read now. A handler
//! cannot know what the form said at the moment of the transition, and
//! ADR-0041 §4 records that as a cost rather than hiding it: the envelope may
//! not carry form data (EES §4.2), and a snapshot beside it would be a second
//! copy of it. `targetStatus` is the status the transition projected — the
//! `mapsToDocumentStatus` of the state it reached, in the pinned revision —
//! because that belongs to the transition rather than to the document.
//!
//! # Nothing to deliver to is a delivery
//!
//! An instance, definition or document that is gone, a transition the pinned
//! revision does not declare, or an empty chain: each is settled `Delivered`
//! with nothing run. None of them is something a retry could change.
//!
//! [#519]: https://github.com/sujanto-gaws/kelir/issues/519

use serde_json::{json, Value};
use uuid::Uuid;

use super::super::domain::Graph;
use super::super::repository::{definition as definition_repo, instance as instance_repo};
use crate::error::AppError;
use crate::modules::hook;
use crate::modules::outbox::Delivery;
use crate::modules::rad::evaluator::RuleEvaluator;

/// The fields of a `Workflow.Transitioned` envelope this consumer reads.
struct Transitioned<'a> {
    instance_id: Uuid,
    document_id: Uuid,
    from: &'a str,
    to: &'a str,
    action: &'a str,
    actor_user_id: Option<Uuid>,
    correlation_id: Option<Uuid>,
}

impl<'a> Transitioned<'a> {
    fn read(envelope: &'a Value) -> Option<Self> {
        let payload = envelope.get("payload")?;
        let uuid = |value: &Value| value.as_str().and_then(|text| text.parse().ok());

        Some(Self {
            instance_id: uuid(payload.get("instanceId")?)?,
            document_id: uuid(payload.get("documentId")?)?,
            from: payload.get("fromState")?.as_str()?,
            to: payload.get("toState")?.as_str()?,
            action: payload.get("action")?.as_str()?,
            // The person, where a person decided it. An automatic step is the
            // engine's, and LHCS §4's `actorUserId` is `null` for it.
            actor_user_id: envelope
                .get("actor")
                .filter(|actor| actor["actorType"] == "USER")
                .and_then(|actor| uuid(&actor["actorId"])),
            correlation_id: envelope.get("correlationId").and_then(uuid),
        })
    }
}

/// Delivers one `Workflow.Transitioned` event to its after-chain.
pub async fn deliver(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    envelope: &Value,
) -> Result<Delivery, AppError> {
    let Some(event) = Transitioned::read(envelope) else {
        // Written by `engine::publish_transition` and nothing else, so a row
        // that does not read is a row written another way. Not retried: the
        // envelope will not read better on the sixth attempt.
        tracing::error!(
            %tenant_id,
            "a Workflow.Transitioned event does not carry the fields its producer writes; \
             it is settled with nothing run"
        );

        return Ok(Delivery::Delivered);
    };

    let Some(instance) =
        instance_repo::find_instance(&mut **transaction, tenant_id, event.instance_id).await?
    else {
        return Ok(Delivery::Delivered);
    };

    let Some(definition) = definition_repo::definition_of_instance(
        &mut **transaction,
        tenant_id,
        instance.workflow_definition_id,
    )
    .await?
    else {
        return Ok(Delivery::Delivered);
    };

    let graph = Graph::parse(&definition.definition_json, definition.version);

    // The first edge the pinned revision declares for this move. Two edges with
    // the same source, target and action differ only in their `condition`, and
    // the event does not say which of them held; S7 makes that a definition
    // with a redundant edge rather than two different transitions.
    let edge = graph.transitions.iter().find(|transition| {
        transition.from == event.from
            && transition.to == event.to
            && transition.action.as_db() == event.action
    });

    let Some(subject) =
        hook::repository::hook_subject(transaction, tenant_id, event.document_id).await?
    else {
        return Ok(Delivery::Delivered);
    };

    let registry = hook::repository::registry_chain(
        transaction,
        tenant_id,
        subject.document_type_id,
        hook::AFTER_WORKFLOW_TRANSITION,
    )
    .await?;

    let chain = hook::service::merge(
        registry,
        edge.map(|edge| edge.actions.clone()).unwrap_or_default(),
    );

    if chain.is_empty() {
        return Ok(Delivery::Delivered);
    }

    let metadata = hook::repository::metadata_of(transaction, tenant_id, event.document_id).await?;
    let transition_ref =
        hook::service::transition_ref(&graph.workflow_key, graph.revision, event.from, event.to);
    let context = hook::service::workflow_context(
        &graph.workflow_key,
        graph.revision,
        instance.id,
        &instance.current_state,
        event.from,
        event.to,
        event.action,
    );

    let mut invocation = hook::Invocation {
        hook_name: hook::AFTER_WORKFLOW_TRANSITION,
        stage: hook::Stage::Transition,
        // Overwritten per entry by the runner.
        source: hook::Source::Workflow,
        tenant_id,
        document_id: event.document_id,
        document_type_key: &subject.document_type_key,
        current_status: Some(&subject.status),
        target_status: graph
            .state(event.to)
            .map(|state| state.maps_to_document_status.as_str()),
        actor_user_id: event.actor_user_id,
        form_data: subject.form_data.clone(),
        metadata,
        workflow_context: context,
        subject: Value::Null,
        config: json!({}),
        correlation_id: event.correlation_id.unwrap_or(instance.id),
    };

    let outcome = hook::service::run_after_chain(
        transaction,
        &RuleEvaluator::new(),
        &chain,
        &mut invocation,
        Some(&transition_ref),
    )
    .await?;

    if outcome.failures.is_empty() {
        Ok(Delivery::Delivered)
    } else {
        Ok(Delivery::Retry(outcome.failures.join("; ")))
    }
}
