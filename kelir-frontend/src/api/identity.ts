import { deleteItem, getItem, getPage, postItem, postVoid, putItem } from './client'
import type { Page, PageQuery } from '@/types/api'
import type {
  CreateRoleRequest,
  CreateUserRequest,
  Permission,
  Role,
  SetPasswordRequest,
  UpdateRoleRequest,
  UpdateUserRequest,
  User,
} from '@/types/identity'

/**
 * The identity administration endpoints (`/api/v1/identity/*`).
 *
 * Thin by design: every function is one call through the shared client, so
 * envelope unwrapping and error normalisation happen in exactly one place
 * (coding standard §3.3). Nothing here composes a message for a user — the
 * component decides what a person reads.
 *
 * Permission codes each call requires, mirroring `identity/service.rs`:
 * `identity:user:read|create|update|delete` and
 * `identity:role:read|create|update|delete`. The backend re-checks all of them;
 * the `can()` gates in the pages only hide what would fail.
 */

const USERS = '/identity/users'
const ROLES = '/identity/roles'

/** Paginated. `page` is 1-based; `pageSize` is clamped server-side to 1..=100. */
export function listUsers(query: PageQuery = {}): Promise<Page<User>> {
  return getPage<User>(USERS, query)
}

export function getUser(id: string): Promise<User> {
  return getItem<User>(`${USERS}/${id}`)
}

/** 201 on success; 409 when the username or email is taken; 422 on field problems. */
export function createUser(request: CreateUserRequest): Promise<User> {
  return postItem<User>(USERS, request)
}

export function updateUser(id: string, request: UpdateUserRequest): Promise<User> {
  return putItem<User>(`${USERS}/${id}`, request)
}

/**
 * Soft-delete: the backend sets `INACTIVE` plus `deleted_at`, so the row leaves
 * the list entirely rather than appearing as inactive. Answers 204.
 *
 * Deactivating yourself is refused with 400, not 403 — the permission check
 * passes first, then the self-check fails.
 */
export function deactivateUser(id: string): Promise<void> {
  return deleteItem(`${USERS}/${id}`)
}

/** Administrative password reset. Answers 204. */
export function setUserPassword(id: string, request: SetPasswordRequest): Promise<void> {
  return postVoid(`${USERS}/${id}/password`, request)
}

export function listRoles(query: PageQuery = {}): Promise<Page<Role>> {
  return getPage<Role>(ROLES, query)
}

export function getRole(id: string): Promise<Role> {
  return getItem<Role>(`${ROLES}/${id}`)
}

/** 409 when `roleCode` is already in use. */
export function createRole(request: CreateRoleRequest): Promise<Role> {
  return postItem<Role>(ROLES, request)
}

export function updateRole(id: string, request: UpdateRoleRequest): Promise<Role> {
  return putItem<Role>(`${ROLES}/${id}`, request)
}

/** 409 with the backend's own wording when the role is a system role. */
export function deleteRole(id: string): Promise<void> {
  return deleteItem(`${ROLES}/${id}`)
}

/**
 * The permission catalogue.
 *
 * Unlike the other collections this answers with an *item* envelope wrapping an
 * array — no `meta`, no pagination — so it goes through `getItem`, not
 * `getPage`, which would reject it as malformed. Guarded by
 * `identity:role:read`, not a `permission:*` code.
 */
export function listPermissions(): Promise<Permission[]> {
  return getItem<Permission[]>('/identity/permissions')
}
