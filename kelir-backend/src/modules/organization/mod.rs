//! Organization: tenants, and later companies, departments, positions and
//! workgroups (architectures/02 §OrganizationModule).
//!
//! Only the tenant slice exists so far, and only the part FR-IDM-009 needs:
//! turning a deployment's configuration and a sign-in request into the
//! `tenant_id` every other module already filters by. Tenant administration —
//! creating, suspending and listing tenants — has no endpoints yet, so there is
//! no `handlers.rs`.

pub mod domain;
pub mod repository;
pub mod service;
