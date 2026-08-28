//! Who a task is for (FR-WF-004, [#176] AC2, AC4).
//!
//! **This is the only place the question is answered.** A second answer written
//! beside it — in the engine, or in a future action handler — would be a second
//! routing rule, and the two would disagree about a task somebody is waiting on.
//!
//! # The seam delegation occupies, and why it is a paragraph rather than a stub
//!
//! [#176] AC4 asks for *"a named seam, documented as the place a delegation
//! window will later apply"*, and **D-13** unscheduled
//! [#24](https://github.com/sujanto-gaws/kelir/issues/24) with a return
//! condition rather than dropping it, on the reasoning that `delegations` has
//! existed since `0002` with nothing reading it and its consumer is this path.
//!
//! JWSS §5.1 already says where delegation goes: *"Delegation windows
//! (`delegations`, Database Schema §3.8) are applied by the assignment resolver
//! **after the rule resolves**; they are not part of the rule."* This file does
//! not invent a seam — it is built so that sentence is implementable:
//!
//! 1. [`normalize`] turns a JWSS rule — object form or §5.2 shorthand, already
//!    unified by [`AssignmentRule`] — into a **principal**: a user, or a role
//!    with an optional department scope.
//! 2. [`direct`] turns a principal into a [`ResolvedAssignment`] against this
//!    tenant's tables.
//!
//! **A delegation window redirects between the two**, and it can do so without
//! either function changing, because a window rewrites *who the principal is* and
//! touches neither the definition's rule nor the columns the task is written to.
//! [#184](https://github.com/sujanto-gaws/kelir/issues/184)'s entry criterion is
//! to confirm this boundary exists before it starts, and this is what it is
//! confirming.
//!
//! **No empty function is written for it.** A no-op named `apply_delegations`
//! would be a reader's first assumption that delegation exists and does nothing,
//! which is worse than a documented gap — it is the failure **D-13** was
//! avoiding when it refused to leave `delegations` a table with a writer and no
//! reader.
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
}

impl ResolvedAssignment {
    fn user(id: Uuid) -> Self {
        Self {
            assignee_user_id: Some(id),
            candidate_role_id: None,
            candidate_department_id: None,
        }
    }

    fn role(role_id: Uuid, department_id: Option<Uuid>) -> Self {
        Self {
            assignee_user_id: None,
            candidate_role_id: Some(role_id),
            candidate_department_id: department_id,
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
/// **It is built on `resolve` rather than beside it**, which is the whole point:
/// `OWNER`, `USER`, `ROLE` and `DEPARTMENT_ROLE` mean the same thing on an edge
/// as they do on a task, because the same function decides what they mean. A
/// second resolver would be a second dialect on the surface that decides who
/// approves an invoice — the failure **D-10** was paid to avoid one layer down.
/// It also inherits [#225](https://github.com/sujanto-gaws/kelir/issues/225)'s
/// fix for free: a `DEPARTMENT_ROLE` edge is checked against both halves of the
/// grant because [`task_repo::holds_role`] is the same one the decision uses.
///
/// **An actorless caller is refused when a rule names anybody.** Nothing
/// reaches here without an actor today — `fire`'s only caller is a decision —
/// and an edge that declares who may take it, taken by nobody identifiable, is
/// the case the declaration exists to prevent. `AUTO` transitions are not an
/// exception waiting to happen: S5 forbids them an `allowedBy` at all, so they
/// arrive here as `None` and never call this.
pub async fn permits(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    rule: &AssignmentRule,
    context: AssignmentContext,
    actor: Option<Uuid>,
    path: &str,
) -> Result<bool, AppError> {
    let Some(actor) = actor else {
        return Ok(false);
    };

    let resolved = resolve(transaction, tenant_id, rule, context, path).await?;

    if resolved.assignee_user_id == Some(actor) {
        return Ok(true);
    }

    match resolved.candidate_role_id {
        Some(role_id) => Ok(task_repo::holds_role(
            &mut **transaction,
            tenant_id,
            actor,
            role_id,
            resolved.candidate_department_id,
        )
        .await?),
        None => Ok(false),
    }
}

/// A rule, resolved to the row a task is written with.
///
/// Runs inside the transition's transaction: the role it names must still exist
/// when the task referencing it is inserted, and the foreign key is what makes
/// that true rather than a lock.
pub async fn resolve(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    rule: &AssignmentRule,
    context: AssignmentContext,
    path: &str,
) -> Result<ResolvedAssignment, AppError> {
    let principal = normalize(rule, context, path)?;

    // A delegation window applies here — see the module documentation. #24,
    // Sprint 11, D-17.

    direct(transaction, tenant_id, principal, path).await
}

/// Who the rule points at, before anything is looked up.
///
/// The step that turns *the definition's words* into *a principal*, and the
/// first half of the seam described at the top of this file. It reads the
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
/// The second half of the seam. It is deliberately incapable of reading a
/// definition: everything the rule said has already been turned into a
/// principal, so a delegation window inserted before it needs to understand
/// principals and not JWSS.
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
}
