-- 0017_tenant_administration.sql — a role grant can no longer cross a tenant
-- boundary (#65), which is what makes tenant administration safe to build.
--
-- Decision D-18 settles the question #65 was filed on and could not answer
-- alone: **roles are tenant-scoped.** Every other identity read already assumed
-- so — `users`, `user_roles` and `roles` are all filtered by `tenant_id` — while
-- `roles_of_user` and `permissions_for_user` joined without the filter, and the
-- first-run bootstrap granted the *system* tenant's `ROLE-ADMIN` through a
-- `user_roles` row carrying the *resolved* tenant's id. Both cannot be right.
--
-- The application half of that decision is a filter on those two queries. This
-- is the half that makes the mistake unrepresentable rather than merely
-- unwritten: a composite foreign key on `(role_id, tenant_id)`, so a grant of
-- another tenant's role is refused by the database whatever forgets to filter.
-- A query can be written wrong again; a constraint cannot.
--
-- `roles` gains `UNIQUE (id, tenant_id)` for no reason other than to be that
-- key's target — `id` is already the primary key, so the constraint adds no
-- guarantee, only an index PostgreSQL requires a composite reference to point
-- at.
--
-- The single-column keys stay. `user_roles.role_id REFERENCES roles (id)` is
-- now implied by the composite one, and dropping it would buy an index and cost
-- the ability to roll this migration's effect back by hand. Redundant is the
-- cheaper of the two.
--
-- **N−1 compatibility.** Additive: no column changes type, and no existing row
-- violates the new keys — every deployment runs one tenant, `SYSTEM`, and every
-- seeded row is in it. The previous release keeps running against this schema
-- with one behavioural difference, and it is the point of the migration: its
-- bootstrap, on a deployment whose `KELIR_DEFAULT_TENANT_CODE` is not `SYSTEM`,
-- now fails loudly where it used to write the cross-boundary grant silently.
-- That is #65 becoming visible rather than a regression.

ALTER TABLE roles
    ADD CONSTRAINT uq_roles_id_tenant_id UNIQUE (id, tenant_id);

-- A user may hold only a role belonging to their own tenant.
ALTER TABLE user_roles
    ADD CONSTRAINT fk_user_roles_role_id_tenant_id
    FOREIGN KEY (role_id, tenant_id) REFERENCES roles (id, tenant_id);

-- And a role may be given permissions only through a row filed under its own
-- tenant. `permission_id` is deliberately *not* paired with `tenant_id` here:
-- the permission catalogue is global and lives in the system tenant (§3.5), so
-- `role_permissions.tenant_id` names the role's tenant, never the permission's.
ALTER TABLE role_permissions
    ADD CONSTRAINT fk_role_permissions_role_id_tenant_id
    FOREIGN KEY (role_id, tenant_id) REFERENCES roles (id, tenant_id);
