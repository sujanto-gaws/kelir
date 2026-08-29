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
//! # A rule that resolves to nobody fails the transition
//!
//! Rather than storing a task with no assignee and no candidate role. An
//! unassignable task is an approval that has stopped, and the moment to find out
//! is the moment it was created — not when somebody eventually asks why a
//! requisition has been sitting for a week. So the transaction rolls back and the
//! caller is told which rule and which value.
//!
//! [#176]: https://github.com/sujanto-gaws/kelir/issues/176

use uuid::Uuid;

use super::super::domain::{AssigneeType, AssignmentRule};
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
    /// The owner's own department, for `OWNER_DEPARTMENT`.
    pub owner_department_id: Option<Uuid>,
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

    let principal = normalize(rule, context, path)?;
    let resolved = direct(transaction, tenant_id, principal, path).await?;

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
/// when the task referencing it is inserted, and the foreign key is what makes
/// that true rather than a lock. The delegation window read below is in the same
/// transaction for a second reason — the window that was open when the task was
/// written is the window the task's `delegated_from_user_id` then claims was
/// open.
pub async fn resolve(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    rule: &AssignmentRule,
    context: AssignmentContext,
    path: &str,
) -> Result<ResolvedAssignment, AppError> {
    let principal = normalize(rule, context, path)?;
    let resolved = direct(transaction, tenant_id, principal, path).await?;

    // JWSS §5.1: **after the rule resolves**, and not part of it.
    redirect(transaction, tenant_id, resolved, context).await
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
) -> Result<Principal, AppError> {
    match rule.assignee_type {
        AssigneeType::User => {
            let raw = rule.user_id.as_deref().unwrap_or_default();

            let id = raw.parse::<Uuid>().map_err(|_| {
                unresolvable(
                    path,
                    "userId",
                    format!("`{raw}` is not a user id; a USER assignment names a user's id"),
                )
            })?;

            Ok(Principal::User(id))
        }
        AssigneeType::Owner => context.owner_user_id.map(Principal::User).ok_or_else(|| {
            unresolvable(
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
                            path,
                            "departmentScope",
                            "this document names no requested department, so \
                             REQUESTED_DEPARTMENT resolves to nothing"
                                .to_owned(),
                        )
                    })?)
                }
                Some("OWNER_DEPARTMENT") => {
                    DepartmentScope::Id(context.owner_department_id.ok_or_else(|| {
                        unresolvable(
                            path,
                            "departmentScope",
                            "the document's creator belongs to no department, so \
                             OWNER_DEPARTMENT resolves to nothing"
                                .to_owned(),
                        )
                    })?)
                }
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
async fn direct(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    principal: Principal,
    path: &str,
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
                return Err(unresolvable(
                    path,
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
            let role_id = sqlx::query_scalar!(
                r#"
                SELECT id FROM roles
                WHERE tenant_id = $1 AND role_code = $2 AND deleted_at IS NULL
                "#,
                tenant_id,
                role_code
            )
            .fetch_optional(&mut **transaction)
            .await?
            .ok_or_else(|| {
                unresolvable(
                    path,
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
                        unresolvable(
                            path,
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

/// The one refusal shape this file raises.
///
/// A 422 rather than a 409 or a 500: the definition names something that does
/// not resolve, which is a property of the stored configuration against this
/// tenant's data. The path points at the *definition's* field so an
/// administrator can find it, and the message names the value rather than saying
/// "unresolvable", because "unresolvable" tells them nothing they can act on.
fn unresolvable(path: &str, field: &str, message: String) -> AppError {
    AppError::validation(vec![ValidationDetail::new(
        format!("{path}.{field}"),
        "assignment",
        "ASSIGNMENT_UNRESOLVED",
        format!(
            "{message}. The task this transition would create would be assigned to \
             nobody, so the transition is refused rather than leaving an approval \
             that has silently stopped"
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
            owner_department_id: Some(Uuid::now_v7()),
        }
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
        let principal =
            normalize(&rule(AssigneeType::Owner), context, "task.assignment").expect("an owner");

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

        let error =
            normalize(&rule(AssigneeType::Owner), context, "task.assignment").expect_err("refused");

        assert_eq!(code(error), "ASSIGNMENT_UNRESOLVED");
    }

    #[test]
    fn a_department_role_reads_its_scope_from_the_document() {
        let context = context();
        let mut department_role = rule(AssigneeType::DepartmentRole);
        department_role.role_code = Some("FINANCE".to_owned());
        department_role.department_scope = Some("REQUESTED_DEPARTMENT".to_owned());

        let principal =
            normalize(&department_role, context, "task.assignment").expect("a scoped role");

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

        let error = normalize(&department_role, context, "task.assignment").expect_err("refused");

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

        let principal =
            normalize(&department_role, context(), "task.assignment").expect("a coded scope");

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

        let error = normalize(&user, context(), "task.assignment").expect_err("refused");

        assert_eq!(code(error), "ASSIGNMENT_UNRESOLVED");
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
