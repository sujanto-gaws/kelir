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
//! because that belongs to the transition rather than to the document. The
//! payload is built by [`after_invocation`], and
//! `the_target_status_is_the_reached_states_document_status` pins that value
//! ([#759]); `a_guard_and_an_after_hook_read_one_transition` pins that a guard
//! on the same transition reads the same one ([#679]).
//!
//! # Nothing to deliver to is a delivery
//!
//! An instance, definition or document that is gone, a transition the pinned
//! revision does not declare, or an empty chain: each is settled `Delivered`
//! with nothing run. None of them is something a retry could change.
//!
//! [#519]: https://github.com/sujanto-gaws/kelir/issues/519
//! [#679]: https://github.com/sujanto-gaws/kelir/issues/679
//! [#759]: https://github.com/sujanto-gaws/kelir/issues/759

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
    .into_loaded() else {
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
    let mut invocation = after_invocation(tenant_id, &event, &graph, &instance, &subject, metadata);

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

/// The LHCS §4 payload the after-chain hands each entry.
///
/// **Pure, and every choice of field is made here** ([#759]): which of
/// `event.to`, `event.from` and `instance.current_state` a field reads is
/// decided inside this function rather than at its call site, so the unit
/// tests below pin it. No core handler is after-capable and also reports what
/// it was given, so before this was a function a wrong `targetStatus` was
/// observable to no test.
///
/// `event` borrows for the payload's own lifetime, which it need not: with
/// its own, #759's mutant `Some(event.to)` is refused by the borrow checker
/// instead of being run, and the test that should go red under it is never
/// asked. Keep it.
///
/// [#759]: https://github.com/sujanto-gaws/kelir/issues/759
fn after_invocation<'a>(
    tenant_id: Uuid,
    event: &Transitioned<'a>,
    graph: &'a Graph,
    instance: &instance_repo::InstanceRow,
    subject: &'a hook::repository::HookSubject,
    metadata: Value,
) -> hook::Invocation<'a> {
    hook::Invocation {
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
        workflow_context: hook::service::workflow_context(
            &graph.workflow_key,
            graph.revision,
            instance.id,
            &instance.current_state,
            event.from,
            event.to,
            event.action,
        ),
        subject: Value::Null,
        config: json!({}),
        correlation_id: event.correlation_id.unwrap_or(instance.id),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn envelope(actor: Value) -> Value {
        json!({
            "correlationId": Uuid::nil().to_string(),
            "actor": actor,
            "payload": {
                "instanceId": Uuid::nil().to_string(),
                "documentId": Uuid::nil().to_string(),
                "fromState": "SUBMITTED",
                "toState": "APPROVED",
                "action": "APPROVE",
            },
        })
    }

    /// **LHCS §4's `actorUserId` is the person who decided**, and `null` for
    /// the engine's own step: the after-chain hands a handler who acted.
    ///
    /// Seen red, 2026-09-25 (Sprint 20 mutation campaign): the `actorType`
    /// test inverted in `Transitioned::read`, which gave the person's step no
    /// actor.
    #[test]
    fn a_decided_transition_carries_its_person_and_an_automatic_one_none() {
        let person = Uuid::now_v7();

        let decided = envelope(json!({ "actorType": "USER", "actorId": person.to_string() }));
        let automatic = envelope(json!({ "actorType": "WORKFLOW_ENGINE", "actorId": null }));

        assert_eq!(
            Transitioned::read(&decided).expect("reads").actor_user_id,
            Some(person)
        );
        assert_eq!(
            Transitioned::read(&automatic).expect("reads").actor_user_id,
            None
        );
    }

    // ── The payload (#759) ────────────────────────────────────────────────
    //
    // One fixture in which every value a field could be read from differs
    // from every other: the reached state's code (`DONE`) from its
    // `mapsToDocumentStatus` (`COMPLETED`), `event.from` from `event.to`, the
    // instance's `current_state` (`FILED`, since it has moved on, LHCS §5.2)
    // from both, and the document's `status` (`IN_REVIEW`) from each of those
    // states' document statuses. A field read from the wrong source cannot
    // land on the right value by coincidence.

    const TYPE_KEY: &str = "Expense-Claim";

    struct Fixture {
        tenant: Uuid,
        document: Uuid,
        type_id: Uuid,
        graph: Graph,
        instance: instance_repo::InstanceRow,
        subject: hook::repository::HookSubject,
    }

    fn fixture() -> Fixture {
        let graph = Graph::parse(
            &json!({
                "workflowKey": "expense_claim",
                "initialState": "SUBMITTED",
                "states": [
                    { "code": "SUBMITTED", "name": "Submitted",
                      "mapsToDocumentStatus": "PENDING_APPROVAL" },
                    { "code": "DONE", "name": "Done",
                      "mapsToDocumentStatus": "COMPLETED", "isFinal": true },
                    { "code": "FILED", "name": "Filed",
                      "mapsToDocumentStatus": "ARCHIVED", "isFinal": true }
                ],
                "transitions": [
                    { "from": "SUBMITTED", "to": "DONE", "action": "AUTO" }
                ]
            }),
            7,
        );
        let document = Uuid::now_v7();
        let type_id = Uuid::now_v7();

        Fixture {
            tenant: Uuid::now_v7(),
            document,
            type_id,
            instance: instance_repo::InstanceRow {
                id: Uuid::now_v7(),
                instance_ref: "WF-2026-000001".to_owned(),
                document_id: document,
                workflow_definition_id: Uuid::now_v7(),
                workflow_key: graph.workflow_key.clone(),
                workflow_name: "Expense claim".to_owned(),
                definition_version: graph.revision,
                status: "RUNNING".to_owned(),
                current_state: "FILED".to_owned(),
                outcome: None,
                business_key: None,
                started_by: None,
                started_at: chrono::Utc::now(),
                completed_at: None,
            },
            graph,
            subject: hook::repository::HookSubject {
                status: "IN_REVIEW".to_owned(),
                form_data: json!({ "amount": 1_250_000 }),
                document_type_id: type_id,
                document_type_key: TYPE_KEY.to_owned(),
            },
        }
    }

    fn event(
        fixture: &Fixture,
        actor_user_id: Option<Uuid>,
        correlation_id: Option<Uuid>,
    ) -> Transitioned<'static> {
        Transitioned {
            instance_id: fixture.instance.id,
            document_id: fixture.document,
            from: "SUBMITTED",
            to: "DONE",
            action: "AUTO",
            actor_user_id,
            correlation_id,
        }
    }

    /// The payload as a handler receives it, less `invokedAt`, which is the
    /// clock's.
    fn payload(invocation: &hook::Invocation<'_>) -> Value {
        let mut payload = invocation.as_json();
        let invoked_at = payload
            .as_object_mut()
            .expect("an object")
            .remove("invokedAt");

        assert!(
            invoked_at.is_some(),
            "LHCS §4 gives every payload `invokedAt`"
        );
        payload
    }

    /// **#759 AC3: `targetStatus` is the reached state's
    /// `mapsToDocumentStatus`**, `COMPLETED`, and not the state code `DONE`
    /// (#759's own mutant), the source state's `PENDING_APPROVAL`, or `null`.
    ///
    /// Before #759 the `event.to` mutant survived every test in the suite
    /// (`tests/workflow_system_tasks.rs`' mutation table), because no core
    /// handler both runs after a transition and reports what it was given.
    #[test]
    fn the_target_status_is_the_reached_states_document_status() {
        let fixture = fixture();
        let event = event(&fixture, None, None);
        let invocation = after_invocation(
            fixture.tenant,
            &event,
            &fixture.graph,
            &fixture.instance,
            &fixture.subject,
            json!({}),
        );

        assert_eq!(payload(&invocation)["targetStatus"], json!("COMPLETED"));
    }

    /// **#759 AC4**: every other field, from its own source, for a person's
    /// step with its own correlation id.
    #[test]
    fn every_field_of_the_after_payload_is_read_from_its_source() {
        let fixture = fixture();
        let person = Uuid::now_v7();
        let correlation = Uuid::now_v7();
        let metadata = json!({ "costCentre": "CC-17" });
        let event = event(&fixture, Some(person), Some(correlation));
        let invocation = after_invocation(
            fixture.tenant,
            &event,
            &fixture.graph,
            &fixture.instance,
            &fixture.subject,
            metadata.clone(),
        );

        assert_eq!(
            payload(&invocation),
            json!({
                "hookName": "after_workflow_transition",
                "stage": "TRANSITION",
                "source": hook::Source::Workflow.as_db(),
                "tenantId": fixture.tenant,
                "documentId": fixture.document,
                "documentTypeKey": TYPE_KEY,
                "currentStatus": "IN_REVIEW",
                "targetStatus": "COMPLETED",
                "actorUserId": person,
                "formData": { "amount": 1_250_000 },
                "metadata": metadata,
                "workflowContext": {
                    "workflowKey": "expense_claim",
                    "workflowRevision": 7,
                    "instanceId": fixture.instance.id,
                    "state": "FILED",
                    "transition": { "from": "SUBMITTED", "to": "DONE", "action": "AUTO" },
                    "taskId": null,
                },
                "subject": null,
                "config": {},
                "correlationId": correlation,
            })
        );
    }

    /// **#759 AC4**: the engine's own step gives no actor, and an envelope
    /// without a correlation id is correlated by the instance.
    #[test]
    fn an_automatic_step_has_no_actor_and_is_correlated_by_its_instance() {
        let fixture = fixture();
        let event = event(&fixture, None, None);
        let invocation = after_invocation(
            fixture.tenant,
            &event,
            &fixture.graph,
            &fixture.instance,
            &fixture.subject,
            json!({}),
        );
        let payload = payload(&invocation);

        assert_eq!(payload["actorUserId"], Value::Null);
        assert_eq!(payload["correlationId"], json!(fixture.instance.id));
    }

    /// **#759 campaign**: a person's decision carries its own action in
    /// `workflowContext.transition`. Every other test here is `AUTO`, so an
    /// action read as the constant `"AUTO"` passed them all.
    #[test]
    fn a_decided_step_carries_its_own_action() {
        let fixture = fixture();
        let decided = Transitioned {
            action: "APPROVE",
            ..event(&fixture, Some(Uuid::now_v7()), None)
        };
        let invocation = after_invocation(
            fixture.tenant,
            &decided,
            &fixture.graph,
            &fixture.instance,
            &fixture.subject,
            json!({}),
        );

        assert_eq!(
            payload(&invocation)["workflowContext"]["transition"],
            json!({ "from": "SUBMITTED", "to": "DONE", "action": "APPROVE" })
        );
    }

    /// **#759 AC6, and #679's agreement at unit level**: for one `AUTO`
    /// transition in one graph and document, a guard and an after-hook read
    /// one value for every field that belongs to the transition.
    ///
    /// `currentStatus`, `formData`, `metadata`, `workflowContext.state` and
    /// `correlationId` are not compared, and that is deliberate: LHCS §5.2
    /// has an after-hook read them at delivery, when the document and the
    /// instance may have moved on, so the two chains are allowed to differ on
    /// them.
    #[test]
    fn a_guard_and_an_after_hook_read_one_transition() {
        use super::super::engine::{document_facts, guard_invocation, EvaluationContext};
        use crate::modules::document::domain::DocumentStatus;

        let fixture = fixture();
        let graph = &fixture.graph;
        let evaluation = EvaluationContext {
            document: document_facts(DocumentStatus::Submitted, fixture.type_id, TYPE_KEY, None),
            form_data: fixture.subject.form_data.clone(),
            variables: json!({}),
            actor: json!({ "userId": null, "roles": [], "departmentId": null }),
        };
        let guard = payload(&guard_invocation(
            fixture.tenant,
            fixture.document,
            graph,
            fixture.instance.id,
            graph.state("SUBMITTED").expect("the source state"),
            &graph.transitions[0],
            graph.state("DONE").expect("the target state"),
            None,
            &evaluation,
            json!({}),
        ));
        let event = event(&fixture, None, None);
        let after = payload(&after_invocation(
            fixture.tenant,
            &event,
            graph,
            &fixture.instance,
            &fixture.subject,
            json!({}),
        ));

        for key in ["targetStatus", "documentTypeKey", "stage", "actorUserId"] {
            assert_eq!(guard[key], after[key], "`{key}`");
        }
        for key in [
            "workflowKey",
            "workflowRevision",
            "instanceId",
            "transition",
        ] {
            assert_eq!(
                guard["workflowContext"][key], after["workflowContext"][key],
                "`workflowContext.{key}`"
            );
        }
        // Equal and wrong would pass the loops above; the values are the
        // transition's.
        assert_eq!(after["targetStatus"], json!("COMPLETED"));
        assert_eq!(after["documentTypeKey"], json!(TYPE_KEY));
    }
}
