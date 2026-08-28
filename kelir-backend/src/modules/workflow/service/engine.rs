//! The one place a process moves (FR-WF-003, 004, 013, 014; [#175]–[#178]).
//!
//! Starting an instance, firing a transition, generating the task the next state
//! declares, and projecting the document's status are **one code path**, called
//! from three places: the submit (§8.1 of the construction plan), the decision
//! ([`super::task`]), and — from Sprint 11 — return, delegate and the rest. An
//! engine is the shape a codebase most easily grows two of, and everything below
//! is arranged so that there is one.
//!
//! # Every function here takes a transaction, and none takes `AppState`
//!
//! Coding standard §2.5: a request MUST NOT hold more than one pooled
//! connection at a time. The submit already runs inside a transaction that holds
//! a numbering counter, and **D-35** is what this project paid to learn that a
//! second connection taken inside one deadlocks the pool at the concurrency the
//! pool can serve. The signatures below refuse it — this is the third time the
//! defect has been written in this codebase and the first time the type system
//! declines to compile it.
//!
//! # The lock ordering, on every path in this module
//!
//! **The instance first, then the task.** [`fire`] takes them in that order and
//! so does everything that calls it. Two paths taking them in opposite orders is
//! a deadlock at exactly the concurrency the feature exists for, and it is a
//! defect no single-threaded test can see — which is why it is a rule stated
//! here rather than an observation about the current code.
//!
//! # Every move through here is recorded, in the same transaction
//!
//! [`history::record`] appends a row for the instance's first state and for
//! every transition after it (FR-WF-012, [#181]). It is written from the
//! caller's transaction because a transition that commits without its history
//! leaves a gap in *how did this document get here* that nothing can see — and
//! `fire` is the one place a process moves, which is what makes "every
//! transition" a property of this file rather than a rule callers must keep.
//!
//! **It is not the audit trail.** [`super::super::repository::history`] carries
//! the distinction in full: this answers how the document got here and is shown
//! to the approver; `audit_events` answers whether it was tampered with and is
//! shown to somebody investigating. Neither is derived from the other.
//!
//! [#181]: https://github.com/sujanto-gaws/kelir/issues/181
//!
//! # A task's `assignment` and a transition's `allowedBy` are two controls
//!
//! Both apply, and neither substitutes for the other. `assignment` says who the
//! *task* is for and is checked by [`super::task::decide`] before it calls in
//! here; `allowedBy` says who may take the *edge* and is checked below, against
//! the transition actually chosen. They coincide in the common shape — a state
//! whose task and whose outgoing transitions name the same role — and JWSS's own
//! example has them differ, with a `RESUBMIT` out of a state that declares no
//! task at all.
//!
//! `allowedBy` was parsed, validated at save and projected to
//! `workflow_transitions.allowed_by_json` and **read by nothing** until
//! [#226](https://github.com/sujanto-gaws/kelir/issues/226). It was latent
//! rather than open: `fire` has one caller, so every transition was reached
//! through a decision the task's own assignment had already gated. The return
//! action is what would have made it live, and this landed first.
//!
//! # `guards` and `actions` are stored and not executed
//!
//! JWSS §7 declares them as hook registration entries merged into the
//! `before_workflow_transition` / `after_workflow_transition` chains. **There is
//! no chain.** `document_lifecycle_hooks` has no reader and architectures/01
//! §12.4.2 is unbuilt, so a definition's handlers are validated for shape,
//! stored, and never invoked. It is said here, once, because a stored handler
//! must not read as evidence that it runs — and when the chain lands, this
//! paragraph is the place the invocation goes.
//!
//! # `AUTO` transitions do not fire
//!
//! For the same reason and with the same honesty: an `AUTO` transition is one
//! the engine fires without a caller, and nothing in Sprint 10 drives one. A
//! definition may declare one — the validator accepts it — and an instance that
//! reaches a state whose only exit is `AUTO` will sit there. S6 catches the
//! version of that which is a dead end; it does not catch a live state waiting
//! for a driver that does not exist, and FR-WF-005 (system tasks) is the `Should`
//! that supplies one.
//!
//! [#175]: https://github.com/sujanto-gaws/kelir/issues/175
//! [#178]: https://github.com/sujanto-gaws/kelir/issues/178

use chrono::{Datelike, Utc};
use serde_json::{json, Value};
use uuid::Uuid;

use super::super::domain::task as task_domain;
use super::super::domain::{
    DecisionAction, Graph, InstanceOutcome, State, TaskStatus, TransitionAction,
};
use super::super::repository::reference::RefKind;
use super::super::repository::{
    definition as definition_repo, history, instance as repo, reference, task,
};
use super::assignment::{self, AssignmentContext};
use crate::error::{AppError, ValidationDetail};
use crate::modules::document::domain::DocumentStatus;
use crate::modules::document::repository as document_repo;
use crate::modules::rad::evaluator::RuleEvaluator;

/// What a caller supplies to start a process.
pub struct StartRequest<'a> {
    pub tenant_id: Uuid,
    pub document_id: Uuid,
    pub workflow_definition_id: Uuid,
    /// The document number, recorded as the instance's business key. `None`
    /// while a document has not taken one, which the submit path never is.
    pub business_key: Option<&'a str>,
    pub actor: Option<Uuid>,
    pub context: AssignmentContext,
    /// What a variable `source` and a transition `condition` evaluate against
    /// (JWSS §6.1).
    pub evaluation: &'a EvaluationContext,
}

/// The context JSON Logic sees (JWSS §6.1).
///
/// Built by the caller because each one holds a different amount of the
/// document — the submit has it locked, a decision has it read — and assembled
/// into the specification's shape here, so the two cannot disagree about what
/// `{"var": "formData.amount"}` addresses.
pub struct EvaluationContext {
    /// The document's own facts. Built by [`document_facts`], which is the one
    /// place their spelling is decided.
    pub document: Value,
    /// The document's form payload — the server's, never the client's.
    pub form_data: Value,
    /// The instance's variables, flat and keyed as the definition declared them.
    /// Empty at instance start, because they are what is being computed.
    pub variables: Value,
}

impl EvaluationContext {
    fn as_json(&self, actor: Option<Uuid>) -> Value {
        json!({
            "document": self.document,
            "formData": self.form_data,
            "variables": self.variables,
            "actor": { "userId": actor },
        })
    }
}

/// The `document` half of JWSS §6.1's condition context.
///
/// **One builder, called by both paths**, so a condition written against
/// `document.status` means the same thing at instance start as it does at the
/// decision that follows. Two call sites each assembling their own object is
/// two vocabularies, and a workflow author would have no way to tell which one
/// their expression was being evaluated against.
///
/// **`document.amount` is absent, and that is a limitation rather than an
/// omission.** JWSS §6.1's own example reads `document.amount`, and
/// `documents.amount` exists as a column "promoted from form data for workflow
/// conditions" (Database Schema §6.6) — **written by nothing**. Until something
/// promotes it, a condition wanting the figure reads `formData.amount`, which
/// is the server's re-evaluated payload and is therefore the more trustworthy of
/// the two anyway. Putting a null `amount` here would be worse than leaving it
/// out: a JSON Logic comparison against null is silently false, so a routing
/// rule would take the wrong branch and report nothing.
pub fn document_facts(
    status: DocumentStatus,
    document_type_id: Uuid,
    document_number: Option<&str>,
) -> Value {
    json!({
        "status": status,
        "documentTypeId": document_type_id,
        "documentNumber": document_number,
    })
}

/// The instance a start produced, and the document status it projected.
pub struct Started {
    pub instance_id: Uuid,
    pub instance_ref: String,
    pub document_status: DocumentStatus,
}

/// Starts an instance of a published definition, in the caller's transaction.
///
/// **Refuses a definition that is not `ACTIVE`**, which is the same rule
/// [#187](https://github.com/sujanto-gaws/kelir/issues/187) enforces at binding
/// time, restated here because a definition can be deprecated between the
/// binding and the submit. Belt and braces on purpose: the binding check is what
/// gives an administrator a message they can act on, and this is what stops a
/// document routing to a revision nobody stands behind.
pub async fn start(
    transaction: &mut sqlx::PgTransaction<'_>,
    request: &StartRequest<'_>,
) -> Result<Started, AppError> {
    let loaded =
        definition_repo::definition_of_instance(&mut **transaction, request.workflow_definition_id)
            .await?
            .ok_or_else(|| AppError::Internal {
                source: anyhow::anyhow!(
                    "document type binds workflow definition {} which does not exist",
                    request.workflow_definition_id
                ),
            })?;

    let (definition_json, workflow_key, _, _) = (
        loaded.definition_json,
        loaded.workflow_key,
        loaded.version,
        loaded.name,
    );

    // The check this function's own documentation claimed and did not make.
    // #187 refuses a binding to anything but an `ACTIVE` definition, and a
    // definition can be deprecated *after* it is bound — at which point every
    // later submission of that type would start a process against a revision
    // nobody stands behind, silently.
    //
    // A 422 rather than an internal error: nothing the caller sent is wrong, and
    // what has to change is the type's binding. The message names the workflow
    // so an administrator has somewhere to go.
    if loaded.status != "ACTIVE" {
        return Err(AppError::validation(vec![ValidationDetail::new(
            "documentTypeId",
            "reference",
            "WORKFLOW_NOT_PUBLISHED",
            format!(
                "this document type routes to workflow `{workflow_key}`, which is {} \
                 rather than published; no approval can be started against it. Bind the \
                 type to a published revision",
                loaded.status
            ),
        )]));
    }

    let graph = Graph::parse(&definition_json);

    let initial = graph.state(&graph.initial_state).ok_or_else(|| {
        // Unreachable through the API: S1 refuses a definition whose initial
        // state is not declared, at save and again at publish. Reachable from a
        // row somebody wrote directly, which is what makes it an internal error
        // rather than a validation failure — nothing the caller sent is wrong.
        AppError::Internal {
            source: anyhow::anyhow!(
                "workflow `{workflow_key}` starts in `{}`, which it does not declare",
                graph.initial_state
            ),
        }
    })?;

    let instance_id = Uuid::now_v7();
    let year = Utc::now().year();
    let instance_ref = reference::allocate(transaction, request.tenant_id, RefKind::Instance, year)
        .await
        .map_err(reference_error)?;

    repo::insert_instance(
        transaction,
        &repo::NewInstance {
            id: instance_id,
            tenant_id: request.tenant_id,
            instance_ref: &instance_ref,
            workflow_definition_id: request.workflow_definition_id,
            document_id: request.document_id,
            business_key: request.business_key,
            current_state: &graph.initial_state,
            started_by: request.actor,
        },
    )
    .await
    .map_err(second_live_instance)?;

    write_variables(transaction, request, &graph, instance_id).await?;

    // **The instance's first row, and the reason the list starts with it.** A
    // history that began at the first *decision* would answer "how did this get
    // here" from halfway: the submit is how it got into the approval at all.
    // `from_state` is null because the initial state came from nowhere, and no
    // action names it because none was taken.
    history::record(
        &mut **transaction,
        &history::NewHistoryEntry {
            tenant_id: request.tenant_id,
            workflow_instance_id: instance_id,
            document_id: request.document_id,
            from_state: None,
            to_state: &graph.initial_state,
            action: None,
            task_id: None,
            comment: None,
            actor_user_id: request.actor,
        },
    )
    .await?;

    let document_status = enter(
        transaction,
        request.tenant_id,
        instance_id,
        request.document_id,
        &graph,
        initial,
        request.actor,
        request.context,
    )
    .await?;

    Ok(Started {
        instance_id,
        instance_ref,
        document_status,
    })
}

/// Where a transition came from, for the history row it writes.
///
/// **A struct rather than two more arguments**, because both are about the same
/// thing — the decision that moved the process — and both are absent together
/// for a transition that no decision drove. `Default` is that case, and naming
/// it is what stops a caller passing `None, None` and meaning nothing in
/// particular.
#[derive(Debug, Clone, Copy, Default)]
pub struct DecisionProvenance<'a> {
    /// The task the decision was recorded against.
    pub task_id: Option<Uuid>,
    /// The reason given with it (FR-TASK-006,
    /// [#182](https://github.com/sujanto-gaws/kelir/issues/182)), already
    /// trimmed and bounded by
    /// [`domain::task::normalize_comment`][task_domain::normalize_comment].
    ///
    /// **`None` and `Some("")` are the same thing and only one of them is
    /// representable here**, which is what `requiresComment` above depends on:
    /// a caller who sent a box full of spaces has given no reason, and an edge
    /// that asks for one must not be satisfied by the space bar.
    pub comment: Option<&'a str>,
}

/// What a decision produced.
pub struct Fired {
    pub from_state: String,
    pub to_state: String,
    pub document_status: DocumentStatus,
    pub outcome: Option<InstanceOutcome>,
}

/// Fires a transition on a locked instance.
///
/// The instance is already held by the caller (`lock_instance`), which is the
/// ordering rule this module keeps: **instance first, then task**.
///
/// The steps, and each of them is one statement or one call:
///
/// 1. Choose the transition — from `current_state`, matching `action`, first
///    whose `condition` holds, **fallback last** (S7, honoured by
///    [`Graph::candidates`]).
/// 2. Move the instance's state, compare-and-swap on the state the choice was
///    made against.
/// 3. Enter the new state: generate its task, or complete the instance.
/// 4. Project the document's status.
#[allow(
    clippy::too_many_arguments,
    reason = "a transition is what it is: an \
    instance, a state, an action, an actor, and the two contexts the definition \
    may read. Bundling them into a struct would name the same fields once more \
    and hide which of them the compare-and-swap depends on"
)]
pub async fn fire(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    instance_id: Uuid,
    document_id: Uuid,
    graph: &Graph,
    from_state: &str,
    action: TransitionAction,
    actor: Option<Uuid>,
    context: AssignmentContext,
    evaluation: &EvaluationContext,
    decision: DecisionProvenance<'_>,
) -> Result<Fired, AppError> {
    let candidates = graph.candidates(from_state, action);

    let chosen = candidates
        .into_iter()
        .find(|transition| match &transition.condition {
            None => true,
            Some(condition) => holds(condition, evaluation, actor),
        })
        .ok_or_else(|| no_transition(graph, from_state, action))?;

    // **`allowedBy` authorizes; it does not select** (#226). The edge is chosen
    // by `condition` — S7, fallback last — and *then* the actor is checked
    // against it. Letting the check pick a different edge instead would mean an
    // approver silently taking a branch the definition did not point them at,
    // which is a worse failure than the refusal: a rejection routed as a return
    // reads as the approver's own decision.
    //
    // Refused as a 403 rather than a 422, and it is the same 403
    // `refuse_unless_theirs` gives one module over. Nothing about the request
    // is malformed — this caller may work tasks, and may work *this* task — so
    // what is wrong is who they are, not what they sent.
    if let Some(rule) = &chosen.allowed_by {
        let path = format!("transitions.{from_state}.{}.allowedBy", action.as_db());

        if !assignment::permits(transaction, tenant_id, rule, context, actor, &path).await? {
            return Err(AppError::Forbidden);
        }
    }

    // **`requiresComment` is checked against the chosen edge, and after the
    // authorization** (JWSS §4.1; FR-TASK-006, #182). Against the *chosen* edge
    // for `allowedBy`'s reason one paragraph up: the definition marks an edge,
    // and `condition` is what picks between two of them — a caller cannot know
    // which they will land on, so neither the client nor the request type can
    // decide this and the engine is where it is decided.
    //
    // After the authorization because the two refusals disclose different
    // things. A 422 saying *this edge wants a reason* is feedback about the
    // payload, and offering it to somebody who may not take the edge at all
    // would answer a question they have not earned. The caller who may take it
    // gets the field-level refusal the screen already applied.
    if chosen.requires_comment && decision.comment.is_none() {
        return Err(task_domain::comment_required(from_state, action.as_db()));
    }

    let target = graph.state(&chosen.to).ok_or_else(|| AppError::Internal {
        source: anyhow::anyhow!(
            "transition to `{}` names a state the definition does not declare",
            chosen.to
        ),
    })?;

    // The outcome is the definition's to say: a final state's
    // `mapsToDocumentStatus` is what ended the process, not the verb that
    // reached it. #178 AC4's rule, applied one field over.
    let outcome = if target.is_final {
        InstanceOutcome::from_document_status(&target.maps_to_document_status)
    } else {
        None
    };

    // **`is_final` is what ends the instance, not the outcome.** They were one
    // argument until a re-read found the gap: a final state mapping to a status
    // no outcome is derived from — `IN_REVIEW`, which the meta-schema permits —
    // produced an instance that stayed `RUNNING` in a state with no exits.
    // Nothing could move it, its document could never be transitioned by hand
    // (the seam refuses while a process is live), and it would hold the
    // one-live-instance index against that document forever. The outcome is
    // *information*; being final is the fact.
    let moved = repo::move_state(
        transaction,
        tenant_id,
        &repo::StateMove {
            id: instance_id,
            from: from_state,
            to: &chosen.to,
            final_state: target.is_final,
            outcome,
        },
        actor,
    )
    .await?;

    if moved == 0 {
        // Somebody else moved it between this transaction's locked read and
        // this statement — which `FOR UPDATE` makes unreachable through this
        // service and does not make unreachable through a future one. A 409
        // rather than a retry: the caller decided against a state the process
        // has left.
        return Err(AppError::conflict(
            "this process moved while the decision was being applied; \
             reload the task and decide again",
        ));
    }

    // Recorded after the move and before the new state is entered, so the row
    // exists whether or not entering succeeds — and inside the same transaction,
    // so it does not exist if the move is rolled back.
    history::record(
        &mut **transaction,
        &history::NewHistoryEntry {
            tenant_id,
            workflow_instance_id: instance_id,
            document_id,
            from_state: Some(from_state),
            to_state: &chosen.to,
            action: Some(action),
            task_id: decision.task_id,
            comment: decision.comment,
            actor_user_id: actor,
        },
    )
    .await?;

    let document_status = enter(
        transaction,
        tenant_id,
        instance_id,
        document_id,
        graph,
        target,
        actor,
        context,
    )
    .await?;

    Ok(Fired {
        from_state: from_state.to_owned(),
        to_state: chosen.to.clone(),
        document_status,
        outcome,
    })
}

/// Enters a state: its task, if it declares one, and the document's status.
///
/// **Both in the caller's transaction** ([#176] AC1, [#178] AC3). A state that
/// says it needs approval and a process with no task to approve it is a stalled
/// instance nobody is told about, and two transactions is how it happens.
///
/// [#176]: https://github.com/sujanto-gaws/kelir/issues/176
#[allow(
    clippy::too_many_arguments,
    reason = "same as `fire`: these are the \
    parts of a transition, and a struct would only rename them"
)]
async fn enter(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    instance_id: Uuid,
    document_id: Uuid,
    graph: &Graph,
    state: &State,
    actor: Option<Uuid>,
    context: AssignmentContext,
) -> Result<DocumentStatus, AppError> {
    if let Some(spec) = &state.task {
        let path = format!("states.{}.task.assignment", state.code);
        let resolved =
            assignment::resolve(transaction, tenant_id, &spec.assignment, context, &path).await?;

        let year = Utc::now().year();
        let task_ref = reference::allocate(transaction, tenant_id, RefKind::Task, year)
            .await
            .map_err(reference_error)?;

        let task_id = Uuid::now_v7();

        task::insert_task(
            transaction,
            &task::NewTask {
                id: task_id,
                tenant_id,
                task_ref: &task_ref,
                workflow_instance_id: instance_id,
                document_id,
                task_definition_key: &spec.task_definition_key,
                task_name: &spec.task_name,
                task_type: &spec.task_type,
                priority: &spec.priority,
                assignee_user_id: resolved.assignee_user_id,
                candidate_role_id: resolved.candidate_role_id,
                candidate_department_id: resolved.candidate_department_id,
                created_by: actor,
            },
        )
        .await
        .map_err(second_open_task)?;

        task::record_task_history(
            transaction,
            tenant_id,
            &task::TaskHistoryEntry {
                task_id,
                instance_id,
                document_id,
                from: None,
                to: if resolved.assignee_user_id.is_some() {
                    TaskStatus::Assigned
                } else {
                    TaskStatus::Created
                },
                action: None,
                actor,
            },
        )
        .await?;
    }

    project_document_status(
        transaction,
        tenant_id,
        document_id,
        &state.maps_to_document_status,
        graph,
    )
    .await
}

/// Writes the document's status from the state the process is in ([#178]).
///
/// **The map is `mapsToDocumentStatus` in the definition and nothing else.** If
/// this function ever grows a `match` from a state code to a status, AC4 has
/// been broken — *a new workflow must not need a backend change to say what its
/// states mean for a document*.
///
/// It is an unconditional `UPDATE` rather than a compare-and-swap, and that is
/// the one place in this module where that is right: the instance's state has
/// just been moved under a lock by a compare-and-swap, and this write is that
/// decision being projected. Guarding it on the document's previous status would
/// make the projection fail when the document is where the workflow says it
/// should be — a conflict with itself.
async fn project_document_status(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    document_id: Uuid,
    maps_to: &str,
    graph: &Graph,
) -> Result<DocumentStatus, AppError> {
    let status = parse_document_status(maps_to).ok_or_else(|| AppError::Internal {
        source: anyhow::anyhow!(
            "workflow `{}` maps a state to `{maps_to}`, which is not a document status",
            graph.workflow_key
        ),
    })?;

    document_repo::set_status_from_workflow(transaction, tenant_id, document_id, status).await?;

    Ok(status)
}

/// Computes and stores the instance's variables (FR-WF-014, [#175] AC2).
///
/// A `source` is JSON Logic over §6.1's context, evaluated through
/// `rad::evaluator` — **the same engine as everything else**, which is **D-10**
/// stated one more time: a second dialect on the surface that decides who
/// approves an invoice would throw away the parity D-10 was bought for.
///
/// **A `source` that cannot be evaluated leaves the variable unset**, rather
/// than failing the submit. That is **D-24**'s answer to the same question — an
/// average over zero rows fails before the first keystroke — applied here: a
/// document submitted at the moment a `source` cannot be computed would be a
/// document whose approval never starts, and the failure would land on somebody
/// who did nothing wrong.
async fn write_variables(
    transaction: &mut sqlx::PgTransaction<'_>,
    request: &StartRequest<'_>,
    graph: &Graph,
    instance_id: Uuid,
) -> Result<(), AppError> {
    if graph.variables.is_empty() {
        return Ok(());
    }

    // `variables` is empty here and cannot be otherwise: this *is* the step that
    // computes them, so a `source` reading another variable would be reading a
    // value that does not exist yet. JWSS gives no ordering between variable
    // declarations, so there is no correct order in which it could.
    let context = request.evaluation.as_json(request.actor);

    // One evaluator for the whole set, which is what `RuleEvaluator::new`'s own
    // documentation asks for: it holds the operator table, and building one per
    // variable would rebuild that table per variable.
    let evaluator = RuleEvaluator::new();
    let mut values = Vec::new();

    for declaration in &graph.variables {
        let Some(source) = &declaration.source else {
            // Declared with no source: nothing computes it at start, and a
            // transition action will when there are transition actions. Storing
            // an empty row for it would be a variable whose value is a lie.
            continue;
        };

        let Ok(value) = evaluator.evaluate(source, &context) else {
            tracing::warn!(
                variable = %declaration.key,
                workflow = %graph.workflow_key,
                "a workflow variable's source could not be evaluated at instance start; \
                 the variable is unset"
            );
            continue;
        };

        let stored = super::super::domain::instance::write_variable(
            &declaration.key,
            &value,
            &declaration.data_type,
        )?;

        values.push((
            declaration.key.clone(),
            declaration.data_type.clone(),
            stored,
        ));
    }

    repo::insert_variables(
        transaction,
        request.tenant_id,
        instance_id,
        &values,
        request.actor,
    )
    .await?;

    Ok(())
}

/// Whether a transition's condition holds.
///
/// A condition that **fails to evaluate is false**, not an error: an unevaluable
/// condition means this branch cannot be shown to apply, and the fallback — the
/// unconditioned transition S7 puts last — is what the process then takes. The
/// alternative is an approval that cannot be decided because of an expression
/// nobody can fix from the screen they are on.
fn holds(condition: &Value, evaluation: &EvaluationContext, actor: Option<Uuid>) -> bool {
    RuleEvaluator::new()
        .evaluate(condition, &evaluation.as_json(actor))
        .map(|value| value.as_bool().unwrap_or(false))
        .unwrap_or(false)
}

fn parse_document_status(value: &str) -> Option<DocumentStatus> {
    // Round-tripped rather than mapped: `from_db` falls back to `Draft` for an
    // unknown value, which would silently turn a typo in a definition into a
    // document sent back to draft. Checking the spelling survives the round trip
    // is what turns that into the internal error it is.
    let parsed = DocumentStatus::from_db(value);

    (parsed.as_db() == value).then_some(parsed)
}

fn no_transition(graph: &Graph, from_state: &str, action: TransitionAction) -> AppError {
    let available: Vec<&str> = graph
        .actions_from(from_state)
        .into_iter()
        .map(|transition| transition.action.as_db())
        .collect();

    let message = if available.is_empty() {
        format!("`{from_state}` is final; nothing moves the process from there")
    } else {
        format!(
            "`{from_state}` has no {} transition whose condition holds. From here: {}",
            action.as_db(),
            available.join(", ")
        )
    };

    AppError::validation(vec![ValidationDetail::new(
        "action",
        "transition",
        "NO_SUCH_TRANSITION",
        message,
    )])
}

/// Turns the partial unique index on live instances into the refusal
/// [#178](https://github.com/sujanto-gaws/kelir/issues/178) AC1 asks for.
///
/// The service refuses a second instance after reading, with a message; this is
/// the layer under it, and it is what makes the refusal true when two submits
/// arrive at once. Coding standard §2.5 requires such a layer to be exercised or
/// declared, and `workflow_seam.rs` reaches it by holding the document's lock in
/// one transaction while the second submit blocks on it.
fn second_live_instance(error: sqlx::Error) -> AppError {
    match &error {
        sqlx::Error::Database(database) if database.is_unique_violation() => AppError::conflict(
            "this document already has a live approval; a second one would mean two \
             processes deciding one document",
        ),
        _ => error.into(),
    }
}

/// Turns the one-open-task-per-instance index into a refusal rather than a 500.
///
/// It should be unreachable: an instance is in one state and a state declares at
/// most one task. It is mapped anyway because "unreachable" is a claim about
/// code that has not been written yet — FR-WF-016 is the requirement that would
/// make it reachable, and this is the message whoever schedules it should see.
fn second_open_task(error: sqlx::Error) -> AppError {
    match &error {
        sqlx::Error::Database(database) if database.is_unique_violation() => AppError::conflict(
            "this process already has an open task; Kelir runs approvals \
             sequentially (FR-WF-016 is unscheduled)",
        ),
        _ => error.into(),
    }
}

fn reference_error(error: sqlx::Error) -> AppError {
    error.into()
}

/// The engine's own vocabulary for the two decisions Sprint 10 issues.
///
/// `DecisionAction` is the API's; this converts it once so the engine's
/// signature stays in terms of the definition's vocabulary rather than the task
/// surface's, which is what lets Sprint 11's return and delegate call `fire`
/// without widening `DecisionAction`.
pub fn transition_of(action: DecisionAction) -> TransitionAction {
    action.transition()
}
