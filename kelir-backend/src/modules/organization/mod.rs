//! Organization: tenants and departments, and later companies, positions and
//! workgroups (architectures/02 §OrganizationModule).
//!
//! Three slices, and the tenant accounts for two of them because it answers two
//! different questions.
//!
//! **Tenant resolution** (`service::resolve_for_sign_in`) turns a deployment's
//! configuration and a sign-in request into the `tenant_id` every other module
//! already filters by — the half FR-IDM-009 needs, shipped in Sprint 4.
//!
//! **Tenant administration** (`service::list_tenants` and its neighbours) is
//! FR-ORG-001, and it arrives with decision **D-18**, which supersedes **D-7**.
//! D-7 deferred it because a deployment served one tenant and the boot guard
//! refused multi-tenant mode, so an administrative surface would have created
//! rows nobody could sign in to. Two things answer that: the guard is gone, and
//! creating a tenant creates its first administrator in the same transaction.
//!
//! The **department** slice (#28, FR-ORG-002) is a full surface, and decision
//! **D-8** makes it the only one: FR-IDM-008 keeps just the edge,
//! `users.department_id`, which the identity module writes. Positions
//! (FR-ORG-003) have no table at all and stay `Could` and unscheduled.

pub mod department;
pub mod department_repository;
pub mod department_service;
pub mod domain;
pub mod handlers;
pub mod repository;
pub mod service;

/// The department permissions, as constants rather than literals — a typo in a
/// permission string is a permission nobody holds, which reads as a working
/// authorization check that refuses everybody (#58).
///
/// **These are `0002_identity.sql`'s rows, not new ones**, and that is worth
/// stating because the obvious move was wrong. Phase 2 seeded
/// `organization:department:read` and `organization:department:manage` and then
/// never built the surface, so for five sprints the catalogue has held two
/// permissions no route checked. Seeding a `create`/`update`/`delete` trio
/// beside them would have left those two orphaned — the same defect as a
/// permission nothing enforces, arrived at from the other direction.
///
/// So the split is coarser than the four this module's siblings use: `read` for
/// reads, `manage` for every write. `manage` is the convention's own word for
/// full control (naming convention §6), and a deployment that needs to separate
/// creating a department from retiring one can have that when someone asks —
/// splitting a permission later is a migration, and it is a smaller one than
/// explaining why two permissions in the catalogue mean nothing.
pub const DEPARTMENT_READ: &str = "organization:department:read";
pub const DEPARTMENT_MANAGE: &str = "organization:department:manage";

/// The tenant permissions, for the same reason, and seeded by the same kind of
/// history: `0005_delegation_tenant_permissions.sql` added them in Sprint 4 for
/// a surface that arrived four sprints later (**D-18**).
pub const TENANT_READ: &str = "organization:tenant:read";
pub const TENANT_MANAGE: &str = "organization:tenant:manage";
