//! Who a task is for (FR-WF-004, [#176] AC2, AC4).
//!
//! **This is the only place the question is answered.** A second answer written
//! beside it — in the engine, or in a future action handler — would be a second
//! routing rule, and the two would disagree about a task somebody is waiting on.
//!
//! # The seam delegation occupies, and what now stands in it
//!
//! [#176] AC4 asked for *"a named seam, documented as the place a delegation
//! window will later apply"*, and **D-13** unscheduled
//! [#24](https://github.com/sujanto-gaws/kelir/issues/24) with a return
//! condition rather than dropping it, on the reasoning that `delegations` had
//! existed since `0002` with nothing reading it and its consumer was this path.
//! [#184] is that return, and this file is where it lands.
//!
//! JWSS §5.1 says where delegation goes: *"Delegation windows (`delegations`,
//! Database Schema §3.8) are applied by the assignment resolver **after the rule
//! resolves**; they are not part of the rule."* Which is why there are three
//! steps here and not one:
//!
//! 1. [`normalize`] turns a JWSS rule — object form or §5.2 shorthand, already
//!    unified by [`AssignmentRule`] — into a **principal**: a user, or a role
//!    with an optional department scope.
//! 2. [`direct`] turns a principal into a [`ResolvedAssignment`] against this
//!    tenant's tables.
//! 3. [`redirect`] applies an open window **between** them, and it can do so
//!    without either function changing, because a window rewrites *who the
//!    principal is* and touches neither the definition's rule nor the columns
//!    the task is written to. That is the whole of what Sprint 10 left named
//!    here, arriving as a value rather than as a redesign.
//!
//! # A window redirects a person's work; it never redirects a role's
//!
//! [`redirect`] applies only where the rule resolved to **a user** — `USER` or
//! `OWNER`. A `ROLE` or `DEPARTMENT_ROLE` assignment produces a task with no
//! assignee, offered to everybody who holds the role, and there is no one
//! person's work in it to hand over: redirecting it would be one holder deciding
//! for all of them, and it would turn a queue item into somebody's task at the
//! moment it was created. `identity::delegation` refuses a `ROLE`-scoped window
//! at the API for the same reason, in the same words.
//!
//! A role holder who has taken such a task **can** still hand it on — that is
//! `POST /workflow/tasks/{id}/delegation`, which acts on a task they hold rather
//! than on a rule.
//!
//! # Routing and authorization are the two questions, and delegation answers
//! them differently
//!
//! [`resolve`] asks *who is this task for* and its answer is a single assignee,
//! so a window **moves** it: during the window the work reaches the delegate and
//! not the delegator. [`permits`] asks *may this person take this edge* and its
//! answer is a yes or a no, so a window **widens** it: the delegate is permitted
//! in addition to the delegator, never instead of them.
//!
//! Which is not a symmetry that was broken for convenience. A delegator holding
//! a task from before the window opened must still be able to decide it —
//! [#184] AC3 is that such tasks do not move — and a `permits` that had followed
//! the window would have refused them on their own approval.
//!
//! **`permits` is told who the actor is standing in for; it does not look it
//! up.** The second party comes from `workflow_tasks.delegated_from_user_id`,
//! which the server wrote, so acting on somebody's behalf is a fact about the
//! task rather than a claim in the request.
//!
//! [#184]: https://github.com/sujanto-gaws/kelir/issues/184
//!
//! # A second way in, for a target a person named
//!
//! [`reassign_to`] (FR-WF-017, [#512]) is an administrator moving an open task
//! to a user or a role they name. **It enters at [`direct`] and skips the other
//! two steps.** There is no rule, so there is nothing to [`normalize`], and no
//! document context to normalize against. There is no [`redirect`] either: a
//! window redirects work a *rule* routes, and a reassign is a person choosing
//! who, which [ADR-0042] records. What it shares is the question that must
//! have one answer, *is this user or role live in this tenant*, and the lock
//! that goes with it. Whether the target could then decide the task is asked
//! of the same functions: [`permits`] for a user, and [`names_role`] for a
//! role.
//!
//! [#512]: https://github.com/sujanto-gaws/kelir/issues/512
//! [ADR-0042]: ../../../../../docs/architectures/adr/0042.%20An%20Administrator%20Reassigns%20an%20Open%20Task%20and%20Nothing%20Cancels%20One.md
//!
//! # A rule that resolves to nobody fails the transition
//!
//! Rather than storing a task with no assignee and no candidate role. An
//! unassignable task is an approval that has stopped, and the moment to find out
//! is the moment it was created — not when somebody eventually asks why a
//! requisition has been sitting for a week. So the transaction rolls back and the
//! caller is told which rule and which value.
//!
//! [#176]: https://github.com/sujanto-gaws/kelir/issues/176

use sqlx::PgExecutor;
use uuid::Uuid;

use super::super::domain::{AssigneeType, AssignmentRule, ReassignTarget};
use super::super::repository::task as task_repo;
use crate::error::{AppError, ValidationDetail};
use crate::modules::identity::delegation_repository as delegation_repo;

/// What the task row's three assignment columns are set to.
///
/// **At most one of `assignee_user_id` and `candidate_role_id` is ever `Some`**,
/// and [`Self::user`] / [`Self::role`] are the only constructors, so there is no
/// way to build one that says both. The reason is
/// [`crate::modules::workflow::domain::task`]'s: an unclaimed role task and a
/// task that is already mine are different situations for the person looking at
/// them, and writing both would erase the difference at creation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedAssignment {
    pub assignee_user_id: Option<Uuid>,
    pub candidate_role_id: Option<Uuid>,
    pub candidate_department_id: Option<Uuid>,
    /// Whose work this is, when a delegation window is why it is going
    /// somewhere else ([#184](https://github.com/sujanto-gaws/kelir/issues/184)
    /// AC4).
    ///
    /// **Never set by the constructors**, which is deliberate: the two of them
    /// answer *who is this for*, and this answers *why them*. It is written by
    /// [`redirect`] and by nothing else, so a resolution that names a delegate
    /// cannot exist without naming the person they are standing in for.
    pub delegated_from_user_id: Option<Uuid>,
}

impl ResolvedAssignment {
    fn user(id: Uuid) -> Self {
        Self {
            assignee_user_id: Some(id),
            candidate_role_id: None,
            candidate_department_id: None,
            delegated_from_user_id: None,
        }
    }

    fn role(role_id: Uuid, department_id: Option<Uuid>) -> Self {
        Self {
            assignee_user_id: None,
            candidate_role_id: Some(role_id),
            candidate_department_id: department_id,
            delegated_from_user_id: None,
        }
    }
}

/// The document facts an assignment rule may refer to.
///
/// Read once by the caller, before the resolver runs, so this function touches
/// no pooled connection of its own — coding standard §2.5, and the reason
/// [`resolve`] takes a transaction rather than an [`AppState`][crate::state::AppState].
#[derive(Debug, Clone, Copy)]
pub struct AssignmentContext {
    /// The type of the document being routed.
    ///
    /// **No assignment rule reads it**, and it is here anyway: a
    /// `DOCUMENT_TYPE`-scoped delegation window covers one type of document, so
    /// the resolver needs to know which one it is holding. Carried on this
    /// struct rather than passed beside it because every caller of [`resolve`]
    /// already builds this from the same document, and a second argument would
    /// be a second chance for the two to disagree.
    pub document_type_id: Uuid,
    /// `documents.created_by` — who raised it. `OWNER` resolves to this.
    pub owner_user_id: Option<Uuid>,
    /// `documents.requested_for_department_id`.
    pub requested_department_id: Option<Uuid>,
    /// The owner's own department, for `OWNER_DEPARTMENT`. Read by
    /// [`AssignmentContext::of_document`] and by nothing else.
    pub owner_department: OwnerDepartment,
}

/// Where the owner's department stands, for `OWNER_DEPARTMENT`
/// ([#579](https://github.com/sujanto-gaws/kelir/issues/579), **D-99** = A).
///
/// **Three cases, because two of them are refused with different words.** An
/// owner with no department and an owner whose department was deleted both
/// resolve to nothing, and an administrator fixes them in different places: the
/// first on the user, the second by restoring the department or moving the
/// user out of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnerDepartment {
    /// The document has no creator, or the creator's `users.department_id` is
    /// empty.
    Unset,
    /// A department of this tenant that is not soft-deleted. `INACTIVE` is
    /// still live here (the product owner's Q3, 2026-09-30).
    Live(Uuid),
    /// `users.department_id` names a department this tenant does not hold
    /// live: soft-deleted, or not this tenant's.
    Gone(Uuid),
}

impl AssignmentContext {
    /// The context a document's assignment rules are resolved against, and
    /// **the one place `OWNER_DEPARTMENT` is read**
    /// ([#579](https://github.com/sujanto-gaws/kelir/issues/579)).
    ///
    /// Every caller that routes or authorizes on a document builds its context
    /// here: the submit's start and resubmit, the decision, and the reassign.
    /// A literal built beside it would be a second answer to *whose department
    /// is this*, and the one this replaces answered `None` at all four.
    ///
    /// # The owner is `documents.created_by`, always
    ///
    /// The parameter is named for the column, and the product owner's Q1
    /// (2026-09-30) is why: the submit passed the **submitter** as the owner,
    /// which is the creator only when the two are one person. `OWNER` and
    /// `OWNER_DEPARTMENT` both follow the creator.
    ///
    /// # The department, and what the read does not check
    ///
    /// **D-99** = A: the owner's `users.department_id` (FR-IDM-008, **D-8**),
    /// not an employee profile's department. The department must be this
    /// tenant's and not soft-deleted. **Its `status` is not read**, so an
    /// `INACTIVE` department still routes (Q3), and **the owner's own
    /// `deleted_at` is not read**, so a document whose creator has since been
    /// removed still routes to the department they were in (Q4).
    ///
    /// # Read at each call, not once
    ///
    /// The product owner's Q2: a task **snapshots** the department into
    /// `workflow_tasks.candidate_department_id` when it is raised, and an
    /// edge's `allowedBy` reads it **live** at each decision and reassign.
    /// A user moved between departments after a task is raised is therefore
    /// measured by the old department at the claim and the task check, and by
    /// the new one at the edge. JWSS §5.3 states this as a known limit.
    pub async fn of_document<'e, E: PgExecutor<'e>>(
        executor: E,
        tenant_id: Uuid,
        document_type_id: Uuid,
        created_by: Option<Uuid>,
        requested_department_id: Option<Uuid>,
    ) -> Result<Self, AppError> {
        let owner_department = match created_by {
            None => OwnerDepartment::Unset,
            Some(owner) => owner_department_of(executor, tenant_id, owner).await?,
        };

        Ok(Self {
            document_type_id,
            owner_user_id: created_by,
            requested_department_id,
            owner_department,
        })
    }
}

/// The owner's `users.department_id`, and whether this tenant holds it live.
///
/// **No `u.deleted_at` and no `d.status`**, which are Q4 and Q3; the doc
/// comment on [`AssignmentContext::of_document`] carries both.
async fn owner_department_of<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    owner: Uuid,
) -> Result<OwnerDepartment, AppError> {
    let row = sqlx::query!(
        r#"
        SELECT u.department_id, d.id AS "live_department_id?"
        FROM users u
        LEFT JOIN departments d
               ON d.id = u.department_id
              AND d.tenant_id = $1
              AND d.deleted_at IS NULL
        WHERE u.tenant_id = $1 AND u.id = $2
        "#,
        tenant_id,
        owner
    )
    .fetch_optional(executor)
    .await?;

    Ok(match row {
        Some(row) => match (row.department_id, row.live_department_id) {
            (Some(_), Some(live)) => OwnerDepartment::Live(live),
            (Some(named), None) => OwnerDepartment::Gone(named),
            (None, _) => OwnerDepartment::Unset,
        },
        None => OwnerDepartment::Unset,
    })
}

/// Whether `actor` satisfies an assignment rule.
///
/// **The other question a rule can be asked.** [`resolve`] asks *who is this
/// for*, and its answer is written onto a task. This asks *may this person take
/// this edge*, and its answer is a yes or a no — [JWSS](../../../../../docs/schema/JSON%20Workflow%20Schema.md)
/// §4's `allowedBy`, which S5 requires on every non-`AUTO` transition and which
/// nothing read until [#226](https://github.com/sujanto-gaws/kelir/issues/226).
///
/// **It is built on the same two steps as `resolve` rather than beside them**,
/// which is the whole point: `OWNER`, `USER`, `ROLE` and `DEPARTMENT_ROLE` mean
/// the same thing on an edge as they do on a task, because the same functions
/// decide what they mean. A second resolver would be a second dialect on the
/// surface that decides who approves an invoice — the failure **D-10** was paid
/// to avoid one layer down. It also inherits
/// [#225](https://github.com/sujanto-gaws/kelir/issues/225)'s fix for free: a
/// `DEPARTMENT_ROLE` edge is checked against both halves of the grant because
/// [`task_repo::holds_role`] is the same one the decision uses.
///
/// **It stops at [`direct`] and does not apply a window**, which is the one
/// place this and `resolve` deliberately differ. The module header carries the
/// reasoning: routing moves, authorization widens.
///
/// # `on_behalf_of` is the second actor, and it comes from the task
///
/// A delegate holds the task because a window put it in their hands or because
/// its holder handed it over, and in both cases
/// `workflow_tasks.delegated_from_user_id` records whose authority they are
/// exercising. An edge naming that person is therefore an edge they may take.
/// Checking the two candidates in turn is what makes [#184] AC5 exact rather
/// than approximate: the delegate is measured against the *delegator's*
/// satisfaction of the rule, so they can do what the delegator could and —
/// since the rule is the only thing consulted — nothing the delegator could not.
///
/// It is a parameter rather than a lookup because the server wrote the column.
/// A resolver that asked *is this actor somebody's delegate* would answer yes
/// for a delegate deciding a task that had never been delegated to them.
///
/// **An actorless caller is refused when a rule names anybody.** Nothing
/// reaches here without an actor today — `fire`'s only caller is a decision —
/// and an edge that declares who may take it, taken by nobody identifiable, is
/// the case the declaration exists to prevent. `AUTO` transitions are not an
/// exception waiting to happen: S5 forbids them an `allowedBy` at all, so they
/// arrive here as `None` and never call this.
///
/// [#184]: https://github.com/sujanto-gaws/kelir/issues/184
pub async fn permits(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    rule: &AssignmentRule,
    context: AssignmentContext,
    actor: Option<Uuid>,
    on_behalf_of: Option<Uuid>,
    path: &str,
) -> Result<bool, AppError> {
    let Some(actor) = actor else {
        return Ok(false);
    };

    let principal = normalize(rule, context, path, Question::Decider)?;
    let resolved = direct(transaction, tenant_id, principal, &|field, message| {
        unresolvable(Question::Decider, path, field, message)
    })
    .await?;

    // The actor first: the ordinary case is somebody deciding their own task,
    // and it costs no query at all when the rule named a user.
    for candidate in [Some(actor), on_behalf_of].into_iter().flatten() {
        if resolved.assignee_user_id == Some(candidate) {
            return Ok(true);
        }

        let Some(role_id) = resolved.candidate_role_id else {
            continue;
        };

        if task_repo::holds_role(
            &mut **transaction,
            tenant_id,
            candidate,
            role_id,
            resolved.candidate_department_id,
        )
        .await?
        {
            return Ok(true);
        }
    }

    Ok(false)
}

/// A rule, resolved to the row a task is written with.
///
/// Runs inside the transition's transaction: the role it names must still exist
/// when the task referencing it is inserted. The foreign key keeps the row, and
/// the `FOR KEY SHARE` in [`direct`] keeps it *live*, which a soft delete would
/// otherwise change underneath the insert (**D-89**). The delegation window
/// read below is in the same transaction for a second reason — the window that
/// was open when the task was written is the window the task's
/// `delegated_from_user_id` then claims was open.
pub async fn resolve(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    rule: &AssignmentRule,
    context: AssignmentContext,
    path: &str,
) -> Result<ResolvedAssignment, AppError> {
    let principal = normalize(rule, context, path, Question::Assignee)?;
    let resolved = direct(transaction, tenant_id, principal, &|field, message| {
        unresolvable(Question::Assignee, path, field, message)
    })
    .await?;

    // JWSS §5.1: **after the rule resolves**, and not part of it.
    redirect(transaction, tenant_id, resolved, context).await
}

/// A reassign's target, resolved to the columns the task is rewritten with
/// ([#512]).
///
/// **A role is named by its code**, as a JWSS rule names it, because [`direct`]
/// resolves a code and a lookup by id beside it would be a second answer to
/// *is this role live*. **No department**: a reassign to a role offers the task
/// to every holder of the role, which [ADR-0042] records.
///
/// **[`direct`], and nothing else.** The target is a principal already, so
/// [`normalize`] has nothing to do, and [`redirect`] is not applied: a window
/// moves the work a rule routes, not a person an administrator chose. The
/// resolution comes from the two constructors, so the task cannot be written
/// naming both a user and a role, and `delegated_from_user_id` is `None`.
///
/// **A role is read `FOR KEY SHARE`**, [`direct`]'s lock, in the reassign's own
/// transaction and before the task is written. `identity::service::delete_role`
/// takes the role `FOR UPDATE` and then counts the open tasks that need it, so
/// the reassign and the delete wait on each other in whichever order they
/// arrive. A delete that commits first leaves the role unmatched, and the
/// reassign is refused. A reassign that commits first is counted.
///
/// [#512]: https://github.com/sujanto-gaws/kelir/issues/512
/// [ADR-0042]: ../../../../../docs/architectures/adr/0042.%20An%20Administrator%20Reassigns%20an%20Open%20Task%20and%20Nothing%20Cancels%20One.md
pub async fn reassign_to(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    target: ReassignTarget,
) -> Result<ResolvedAssignment, AppError> {
    let principal = match target {
        ReassignTarget::User(id) => Principal::User(id),
        ReassignTarget::Role(role_code) => Principal::Role {
            role_code,
            department: None,
        },
    };

    direct(transaction, tenant_id, principal, &not_reassignable).await
}

/// Whether a rule names `role_id`, so that the role's holders are who it
/// admits ([#512], the product owner's decision of 2026-09-26).
///
/// **The role-target half of the reassign's decidability check.** A user
/// target is measured with [`permits`], which needs a person. A role target is
/// a set of people who may change after the reassign, so the question asked of
/// it is whether the rule is *about* that role: a `ROLE` or `DEPARTMENT_ROLE`
/// rule whose role resolves, through [`normalize`] and [`direct`], to this
/// one. A `DEPARTMENT_ROLE` rule counts when its department resolves too; which
/// holders are in the department is then a question about each holder, which
/// the claim and the decision ask.
///
/// **`OWNER` and `USER` rules never name a role**, so a role target does not
/// satisfy them: holding a role does not make anybody the document's creator
/// or the user a rule names. [ADR-0042] records the rule.
///
/// [#512]: https://github.com/sujanto-gaws/kelir/issues/512
/// [ADR-0042]: ../../../../../docs/architectures/adr/0042.%20An%20Administrator%20Reassigns%20an%20Open%20Task%20and%20Nothing%20Cancels%20One.md
pub async fn names_role(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    rule: &AssignmentRule,
    context: AssignmentContext,
    role_id: Uuid,
    path: &str,
) -> Result<bool, AppError> {
    if !matches!(
        rule.assignee_type,
        AssigneeType::Role | AssigneeType::DepartmentRole
    ) {
        return Ok(false);
    }

    // An edge's `allowedBy` is who may decide, so its refusal is a decider's.
    let principal = normalize(rule, context, path, Question::Decider)?;
    let resolved = direct(transaction, tenant_id, principal, &|field, message| {
        unresolvable(Question::Decider, path, field, message)
    })
    .await?;

    Ok(resolved.candidate_role_id == Some(role_id))
}

/// Holds every role that decides a task being raised, and refuses the task if
/// one is gone ([#509], **D-89**).
///
/// `edges` are the `(path, rule)` pairs of the transitions leaving the state
/// the task is raised in. Each `ROLE` or `DEPARTMENT_ROLE` among them names a
/// role a decision on the task will resolve again, through [`permits`].
///
/// # Why a task's own role is not enough
///
/// `identity::service::delete_role` counts the open tasks that need a role
/// under `FOR UPDATE` on the role row, and counts a task whose edges name the
/// role as well as one offered to it. [`direct`] makes a task **offered** to the
/// role wait for that lock. A task offered to *another* role, whose edges name
/// this one, read nothing here before [#509], so it did not wait. It committed
/// alongside the delete, and every decision on it was refused.
///
/// **So each edge's role is read `FOR KEY SHARE` too**, for [`direct`]'s
/// reason. A delete in progress makes this wait, and a role it deleted no
/// longer matches when the wait ends.
///
/// # A role that is gone refuses the transition
///
/// That is the lock's other half, since a lock on a row that no longer matches
/// holds nothing. It is also [`direct`]'s rule for a task's own role, applied to
/// the roles that decide it: a task raised where a decision is already
/// certain to be refused is an approval that has silently stopped. **One edge
/// is enough.** A state whose `APPROVE` names a live role and whose `REJECT`
/// names a deleted one would raise a task that can be approved and never
/// rejected, and the delete count treats that task as needing both roles.
///
/// Before [#509] this refusal came at the decision, and the document had
/// already been submitted.
///
/// [#509]: https://github.com/sujanto-gaws/kelir/issues/509
pub async fn hold_deciding_roles<'a>(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    edges: impl IntoIterator<Item = (String, &'a AssignmentRule)>,
) -> Result<(), AppError> {
    // Once per role however many edges name it, in a stable order.
    let mut roles = std::collections::BTreeMap::new();

    for (path, rule) in edges {
        if !matches!(
            rule.assignee_type,
            AssigneeType::Role | AssigneeType::DepartmentRole
        ) {
            continue;
        }

        let role_code = rule.role_code.clone().unwrap_or_default();
        roles.entry(role_code).or_insert(path);
    }

    for (role_code, path) in roles {
        let live = sqlx::query_scalar!(
            r#"
            SELECT id FROM roles
            WHERE tenant_id = $1 AND role_code = $2 AND deleted_at IS NULL
            FOR KEY SHARE
            "#,
            tenant_id,
            role_code
        )
        .fetch_optional(&mut **transaction)
        .await?;

        if live.is_none() {
            return Err(AppError::validation(vec![ValidationDetail::new(
                format!("{path}.roleCode"),
                "assignment",
                "ASSIGNMENT_UNRESOLVED",
                format!(
                    "`{role_code}` is not a live role in this tenant. Every decision this \
                     edge offers on the task this transition would create would be refused, \
                     so the transition is refused rather than raising a task nobody can decide"
                ),
            )]));
        }
    }

    Ok(())
}

/// An open window applied to a resolution ([#184] AC2).
///
/// **Returns the resolution unchanged in every case but one**, and each of those
/// cases is a decision rather than a fall-through:
///
/// * A resolution with no assignee is a role task, and the module header says
///   why a window does not touch one.
/// * No open window for this person, this document type, right now — the whole
///   of that predicate is [`active_delegate_of`][repo], stated once in SQL
///   rather than half here. AC6's *immediately* is a property of there being
///   nothing between that statement and this task: an expired or switched-off
///   window stops routing on the next transition, not on the next sweep.
///
/// **One hop, and the delegate is not looked up again.** A window whose delegate
/// is not an active user does not match at all, so the person named here has
/// already been checked by the statement that produced them — re-running
/// [`direct`] over the delegate would be a second query asking a weaker
/// question, since `direct` checks only that the row is not deleted.
///
/// [#184]: https://github.com/sujanto-gaws/kelir/issues/184
/// [repo]: crate::modules::identity::delegation_repository::active_delegate_of
async fn redirect(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    resolved: ResolvedAssignment,
    context: AssignmentContext,
) -> Result<ResolvedAssignment, AppError> {
    let Some(delegator) = resolved.assignee_user_id else {
        return Ok(resolved);
    };

    let Some(delegate) = delegation_repo::active_delegate_of(
        &mut **transaction,
        tenant_id,
        delegator,
        Some(context.document_type_id),
    )
    .await?
    else {
        return Ok(resolved);
    };

    Ok(ResolvedAssignment {
        assignee_user_id: Some(delegate),
        delegated_from_user_id: Some(delegator),
        ..resolved
    })
}

/// Who the rule points at, before anything is looked up.
///
/// The step that turns *the definition's words* into *a principal*, and the
/// first of the three described at the top of this file. It reads the
/// document's context and nothing else, so it is pure and testable without a
/// database — which is what makes the `OWNER` arm's failure case assertable.
fn normalize(
    rule: &AssignmentRule,
    context: AssignmentContext,
    path: &str,
    question: Question,
) -> Result<Principal, AppError> {
    match rule.assignee_type {
        AssigneeType::User => {
            let raw = rule.user_id.as_deref().unwrap_or_default();

            let id = raw.parse::<Uuid>().map_err(|_| {
                unresolvable(
                    question,
                    path,
                    "userId",
                    format!("`{raw}` is not a user id; a USER assignment names a user's id"),
                )
            })?;

            Ok(Principal::User(id))
        }
        AssigneeType::Owner => context.owner_user_id.map(Principal::User).ok_or_else(|| {
            unresolvable(
                question,
                path,
                "assigneeType",
                "this document has no creator recorded, so OWNER resolves to nobody".to_owned(),
            )
        }),
        AssigneeType::Role => Ok(Principal::Role {
            role_code: rule.role_code.clone().unwrap_or_default(),
            department: None,
        }),
        AssigneeType::DepartmentRole => {
            let scope = rule.department_scope.as_deref();

            let department = match scope {
                None => DepartmentScope::None,
                Some("REQUESTED_DEPARTMENT") => {
                    DepartmentScope::Id(context.requested_department_id.ok_or_else(|| {
                        unresolvable(
                            question,
                            path,
                            "departmentScope",
                            "this document names no requested department, so \
                             REQUESTED_DEPARTMENT resolves to nothing"
                                .to_owned(),
                        )
                    })?)
                }
                Some("OWNER_DEPARTMENT") => match context.owner_department {
                    OwnerDepartment::Live(id) => DepartmentScope::Id(id),
                    OwnerDepartment::Unset => {
                        return Err(unresolvable(
                            question,
                            path,
                            "departmentScope",
                            "the document's creator belongs to no department, so \
                             OWNER_DEPARTMENT resolves to nothing"
                                .to_owned(),
                        ))
                    }
                    OwnerDepartment::Gone(id) => {
                        return Err(unresolvable(
                            question,
                            path,
                            "departmentScope",
                            format!(
                                "the document's creator belongs to department {id}, which \
                                 is not a live department in this tenant, so \
                                 OWNER_DEPARTMENT resolves to nothing"
                            ),
                        ))
                    }
                },
                // Anything else is a department **code**, per JWSS §5.1. Looked
                // up rather than guessed at: a code that names nothing is
                // refused below rather than silently widening the task to the
                // whole role.
                Some(code) => DepartmentScope::Code(code.to_owned()),
            };

            Ok(Principal::Role {
                role_code: rule.role_code.clone().unwrap_or_default(),
                department: Some(department),
            })
        }
    }
}

/// A principal, resolved against this tenant's tables.
///
/// The middle step. It is deliberately incapable of reading a definition:
/// everything the rule said has already been turned into a principal, so
/// [`redirect`] — which runs on the far side of it — needs to understand people
/// and not JWSS.
///
/// `refuse` builds the refusal for a principal that does not resolve, given the
/// field and what is wrong with it. The checks are this function's and the
/// words are the caller's: a rule points at a definition's field and a
/// transition, and [`reassign_to`] at a request's field and a task.
async fn direct(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    principal: Principal,
    refuse: &(dyn Fn(&str, String) -> AppError + Sync),
) -> Result<ResolvedAssignment, AppError> {
    match principal {
        Principal::User(id) => {
            let exists = sqlx::query_scalar!(
                r#"
                SELECT 1 AS "found!" FROM users
                WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL
                "#,
                tenant_id,
                id
            )
            .fetch_optional(&mut **transaction)
            .await?;

            if exists.is_none() {
                return Err(refuse(
                    "userId",
                    format!("user {id} is not a live user in this tenant"),
                ));
            }

            Ok(ResolvedAssignment::user(id))
        }
        Principal::Role {
            role_code,
            department,
        } => {
            // `FOR KEY SHARE` holds the role until the task naming it is
            // committed (**D-89**, #487). The foreign key alone does not: a
            // soft delete is an `UPDATE` that leaves the key alone, so it passes
            // straight through the key-share lock the insert takes, and a delete
            // counting open tasks cannot see one that is not committed yet.
            // `identity::service::delete_role` locks the row `FOR UPDATE`, which
            // this waits on, and a role deleted meanwhile no longer matches when
            // the wait ends. Transitions do not block one another.
            let role_id = sqlx::query_scalar!(
                r#"
                SELECT id FROM roles
                WHERE tenant_id = $1 AND role_code = $2 AND deleted_at IS NULL
                FOR KEY SHARE
                "#,
                tenant_id,
                role_code
            )
            .fetch_optional(&mut **transaction)
            .await?
            .ok_or_else(|| {
                refuse(
                    "roleCode",
                    format!("`{role_code}` is not a live role in this tenant"),
                )
            })?;

            let department_id = match department {
                None | Some(DepartmentScope::None) => None,
                Some(DepartmentScope::Id(id)) => Some(id),
                Some(DepartmentScope::Code(code)) => Some(
                    sqlx::query_scalar!(
                        r#"
                        SELECT id FROM departments
                        WHERE tenant_id = $1 AND department_code = $2 AND deleted_at IS NULL
                        "#,
                        tenant_id,
                        code
                    )
                    .fetch_optional(&mut **transaction)
                    .await?
                    .ok_or_else(|| {
                        refuse(
                            "departmentScope",
                            format!("`{code}` is not a live department in this tenant"),
                        )
                    })?,
                ),
            };

            Ok(ResolvedAssignment::role(role_id, department_id))
        }
    }
}

/// Who a rule points at, with the definition's vocabulary already gone.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Principal {
    User(Uuid),
    Role {
        role_code: String,
        department: Option<DepartmentScope>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DepartmentScope {
    None,
    Id(Uuid),
    Code(String),
}

/// Which of the two questions a rule is being asked, which is what its refusal
/// tells the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Question {
    /// [`resolve`]: who a task being raised is for.
    Assignee,
    /// [`permits`]: who may take an edge. No task is necessarily being raised —
    /// a decision into a final state raises none ([#534]).
    ///
    /// [#534]: https://github.com/sujanto-gaws/kelir/issues/534
    Decider,
}

/// The one refusal shape this file raises.
///
/// A 422 rather than a 409 or a 500: the definition names something that does
/// not resolve, which is a property of the stored configuration against this
/// tenant's data. The path points at the *definition's* field so an
/// administrator can find it, and the message names the value rather than saying
/// "unresolvable", because "unresolvable" tells them nothing they can act on.
///
/// **One code, two reasons.** The value is unresolvable either way, but what it
/// costs differs: an `assignment` would raise a task offered to nobody, and an
/// `allowedBy` leaves a decision nobody may take. Telling the second caller
/// about a task that was never going to exist sends them looking for it.
fn unresolvable(question: Question, path: &str, field: &str, message: String) -> AppError {
    let reason = match question {
        Question::Assignee => {
            "The task this transition would create would be assigned to nobody, so the \
             transition is refused rather than leaving an approval that has silently stopped"
        }
        Question::Decider => {
            "Nobody satisfies this edge's `allowedBy`, so nobody may take this decision, \
             and it is refused rather than let through unchecked"
        }
    };

    AppError::validation(vec![ValidationDetail::new(
        format!("{path}.{field}"),
        "assignment",
        "ASSIGNMENT_UNRESOLVED",
        format!("{message}. {reason}"),
    )])
}

/// [`unresolvable`]'s shape, for a reassign's target.
///
/// The same code, so a client handles one refusal. The path is the request's
/// own field, `userId` or `roleCode`, and the message says the task keeps its
/// holder, because nothing was written.
fn not_reassignable(field: &str, message: String) -> AppError {
    AppError::validation(vec![ValidationDetail::new(
        field,
        "assignment",
        "ASSIGNMENT_UNRESOLVED",
        format!(
            "{message}. A task is reassigned only to somebody who can act on it, so it \
             stays with whoever holds it now"
        ),
    )])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(assignee_type: AssigneeType) -> AssignmentRule {
        AssignmentRule {
            assignee_type,
            user_id: None,
            role_code: None,
            department_scope: None,
        }
    }

    fn context() -> AssignmentContext {
        AssignmentContext {
            document_type_id: Uuid::now_v7(),
            owner_user_id: Some(Uuid::now_v7()),
            requested_department_id: Some(Uuid::now_v7()),
            owner_department: OwnerDepartment::Live(Uuid::now_v7()),
        }
    }

    fn owner_department_rule() -> AssignmentRule {
        let mut department_role = rule(AssigneeType::DepartmentRole);
        department_role.role_code = Some("FINANCE".to_owned());
        department_role.department_scope = Some("OWNER_DEPARTMENT".to_owned());
        department_role
    }

    fn refusal(error: AppError) -> (String, String, String) {
        match error {
            AppError::Validation { details } => (
                details[0].code.clone(),
                details[0].path.clone(),
                details[0].message.clone(),
            ),
            other => panic!("expected a validation error, got {other:?}"),
        }
    }

    /// `OWNER_DEPARTMENT` reads the owner's department, **not** the requested
    /// one ([#579]). The context holds a different id in each slot, so an arm
    /// reading the wrong one names the wrong department.
    ///
    /// [#579]: https://github.com/sujanto-gaws/kelir/issues/579
    #[test]
    fn an_owner_department_scope_reads_the_owners_department() {
        let context = context();

        let principal = normalize(
            &owner_department_rule(),
            context,
            "task.assignment",
            Question::Assignee,
        )
        .expect("a scoped role");

        let OwnerDepartment::Live(owner) = context.owner_department else {
            panic!("the fixture is live");
        };
        assert_eq!(
            principal,
            Principal::Role {
                role_code: "FINANCE".to_owned(),
                department: Some(DepartmentScope::Id(owner)),
            }
        );
    }

    #[test]
    fn an_owner_with_no_department_is_refused_at_the_scope() {
        let context = AssignmentContext {
            owner_department: OwnerDepartment::Unset,
            ..context()
        };

        let (code, path, message) = refusal(
            normalize(
                &owner_department_rule(),
                context,
                "task.assignment",
                Question::Assignee,
            )
            .expect_err("refused"),
        );

        assert_eq!(code, "ASSIGNMENT_UNRESOLVED");
        assert_eq!(path, "task.assignment.departmentScope");
        assert!(
            message.starts_with("the document's creator belongs to no department"),
            "{message}"
        );
    }

    /// A deleted department is named, so the administrator knows which one to
    /// restore or move the owner out of.
    #[test]
    fn an_owner_whose_department_is_gone_is_refused_naming_it() {
        let gone = Uuid::now_v7();
        let context = AssignmentContext {
            owner_department: OwnerDepartment::Gone(gone),
            ..context()
        };

        let (code, path, message) = refusal(
            normalize(
                &owner_department_rule(),
                context,
                "transitions.A.APPROVE.allowedBy",
                Question::Decider,
            )
            .expect_err("refused"),
        );

        assert_eq!(code, "ASSIGNMENT_UNRESOLVED");
        assert_eq!(path, "transitions.A.APPROVE.allowedBy.departmentScope");
        assert!(
            message.starts_with(&format!(
                "the document's creator belongs to department {gone}, which is not a live \
                 department in this tenant"
            )),
            "{message}"
        );
        assert!(
            message.contains("nobody may take this decision"),
            "{message}"
        );
    }

    /// **Every `AssignmentContext` outside this file comes from
    /// [`AssignmentContext::of_document`]** ([#579] criterion 2).
    ///
    /// The four sites each wrote `owner_department_id: None` in a literal of
    /// their own, which is how `OWNER_DEPARTMENT` published cleanly and then
    /// resolved to nothing everywhere. A literal added beside the constructor
    /// would be that again, so the source is read for one.
    ///
    /// [#579]: https://github.com/sujanto-gaws/kelir/issues/579
    #[test]
    fn every_assignment_context_is_built_by_the_one_reader() {
        fn walk(dir: &std::path::Path, found: &mut Vec<(String, String)>) {
            for entry in std::fs::read_dir(dir).expect("read the source tree") {
                let path = entry.expect("an entry").path();
                if path.is_dir() {
                    walk(&path, found);
                } else if path.extension().is_some_and(|extension| extension == "rs") {
                    let text = std::fs::read_to_string(&path).expect("read a source file");
                    found.push((path.display().to_string().replace('\\', "/"), text));
                }
            }
        }

        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = Vec::new();
        walk(&root, &mut files);

        let mut literals = Vec::new();
        let mut readers = 0;

        for (path, text) in &files {
            if path.ends_with("workflow/service/assignment.rs") {
                continue;
            }
            readers += text.matches("AssignmentContext::of_document(").count();
            if text.contains("AssignmentContext {") {
                literals.push(path.clone());
            }
        }

        assert!(
            literals.is_empty(),
            "an AssignmentContext is built by hand outside the reader: {literals:?}"
        );
        assert_eq!(
            readers, 4,
            "the submit's start and resubmit, the decision and the reassign each read \
             the context once"
        );
    }

    fn code(error: AppError) -> String {
        match error {
            AppError::Validation { details } => details[0].code.clone(),
            other => panic!("expected a validation error, got {other:?}"),
        }
    }

    #[test]
    fn owner_resolves_to_the_person_who_raised_the_document() {
        let context = context();
        let principal = normalize(
            &rule(AssigneeType::Owner),
            context,
            "task.assignment",
            Question::Assignee,
        )
        .expect("an owner");

        assert_eq!(
            principal,
            Principal::User(context.owner_user_id.expect("set"))
        );
    }

    #[test]
    fn owner_on_a_document_with_no_creator_is_refused_rather_than_left_unassigned() {
        // The failure this whole file is arranged to make loud. A task written
        // with no assignee is an approval that has stopped, and nobody is told.
        let context = AssignmentContext {
            owner_user_id: None,
            ..context()
        };

        let error = normalize(
            &rule(AssigneeType::Owner),
            context,
            "task.assignment",
            Question::Assignee,
        )
        .expect_err("refused");

        assert_eq!(code(error), "ASSIGNMENT_UNRESOLVED");
    }

    #[test]
    fn a_department_role_reads_its_scope_from_the_document() {
        let context = context();
        let mut department_role = rule(AssigneeType::DepartmentRole);
        department_role.role_code = Some("FINANCE".to_owned());
        department_role.department_scope = Some("REQUESTED_DEPARTMENT".to_owned());

        let principal = normalize(
            &department_role,
            context,
            "task.assignment",
            Question::Assignee,
        )
        .expect("a scoped role");

        assert_eq!(
            principal,
            Principal::Role {
                role_code: "FINANCE".to_owned(),
                department: Some(DepartmentScope::Id(
                    context.requested_department_id.expect("set")
                )),
            }
        );
    }

    #[test]
    fn a_scope_the_document_cannot_supply_is_refused() {
        let context = AssignmentContext {
            requested_department_id: None,
            ..context()
        };
        let mut department_role = rule(AssigneeType::DepartmentRole);
        department_role.role_code = Some("FINANCE".to_owned());
        department_role.department_scope = Some("REQUESTED_DEPARTMENT".to_owned());

        let error = normalize(
            &department_role,
            context,
            "task.assignment",
            Question::Assignee,
        )
        .expect_err("refused");

        assert_eq!(code(error), "ASSIGNMENT_UNRESOLVED");
    }

    #[test]
    fn an_unrecognised_scope_is_a_department_code_rather_than_an_error() {
        // JWSS §5.1: `departmentScope` is one of the two keywords **or** a
        // department code. Treating an unknown value as an error here would
        // make the third case unusable; it is resolved in `direct` and refused
        // there if it names nothing.
        let mut department_role = rule(AssigneeType::DepartmentRole);
        department_role.role_code = Some("FINANCE".to_owned());
        department_role.department_scope = Some("DEPT-PROC".to_owned());

        let principal = normalize(
            &department_role,
            context(),
            "task.assignment",
            Question::Assignee,
        )
        .expect("a coded scope");

        assert_eq!(
            principal,
            Principal::Role {
                role_code: "FINANCE".to_owned(),
                department: Some(DepartmentScope::Code("DEPT-PROC".to_owned())),
            }
        );
    }

    #[test]
    fn a_user_assignment_whose_id_is_not_a_uuid_is_refused() {
        let mut user = rule(AssigneeType::User);
        user.user_id = Some("the finance manager".to_owned());

        let error = normalize(&user, context(), "task.assignment", Question::Assignee)
            .expect_err("refused");

        assert_eq!(code(error), "ASSIGNMENT_UNRESOLVED");
    }

    /// `OWNER` on a document with no creator: the one refusal `normalize` can
    /// raise without a database, asked as either question.
    fn ownerless(question: Question, path: &str) -> (String, String) {
        let context = AssignmentContext {
            owner_user_id: None,
            ..context()
        };

        match normalize(&rule(AssigneeType::Owner), context, path, question) {
            Err(AppError::Validation { details }) => {
                (details[0].code.clone(), details[0].message.clone())
            }
            other => panic!("expected a validation error, got {other:?}"),
        }
    }

    /// **A task's assignment that resolves to nobody is told about the task**,
    /// which is what it would have raised.
    #[test]
    fn an_unresolvable_assignment_is_refused_for_the_task_it_would_raise() {
        let (code, message) = ownerless(Question::Assignee, "states.0.task.assignment");

        assert_eq!(code, "ASSIGNMENT_UNRESOLVED");
        assert!(
            message.contains("The task this transition would create would be assigned to nobody"),
            "{message}"
        );
        assert!(!message.contains("decision"), "{message}");
    }

    /// **An `allowedBy` that resolves to nobody is told about the decision**
    /// ([#534]). A decision into a final state raises no task, and before this
    /// it was told about one.
    ///
    /// [#534]: https://github.com/sujanto-gaws/kelir/issues/534
    #[test]
    fn an_unresolvable_allowed_by_is_refused_for_the_decision_not_a_task() {
        let (code, message) = ownerless(
            Question::Decider,
            "transitions.MANAGER_APPROVAL.APPROVE.allowedBy",
        );

        assert_eq!(code, "ASSIGNMENT_UNRESOLVED");
        assert!(
            message.contains("nobody may take this decision"),
            "{message}"
        );
        assert!(!message.contains("task"), "{message}");
    }

    #[test]
    fn a_resolved_assignment_never_names_both_a_user_and_a_role() {
        // There is no constructor that could produce one, which is the point of
        // there being only two. This asserts the property a reader would
        // otherwise have to check by reading every call site.
        let user = ResolvedAssignment::user(Uuid::now_v7());
        assert!(user.candidate_role_id.is_none());

        let role = ResolvedAssignment::role(Uuid::now_v7(), None);
        assert!(role.assignee_user_id.is_none());
    }

    #[test]
    fn a_freshly_resolved_assignment_names_nobody_it_is_standing_in_for() {
        // `redirect` is the only writer of the field, which is what makes
        // "assigned to a delegate" and "assigned to the person the rule named"
        // distinguishable on the task row. A constructor that set it would let
        // a resolution claim a delegation that never happened.
        assert!(ResolvedAssignment::user(Uuid::now_v7())
            .delegated_from_user_id
            .is_none());
        assert!(ResolvedAssignment::role(Uuid::now_v7(), None)
            .delegated_from_user_id
            .is_none());
    }
}
